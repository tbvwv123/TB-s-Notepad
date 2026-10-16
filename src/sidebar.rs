//! 왼쪽 사이드바: 폴더 › 하위 폴더 › 메모 파일 트리, 검색, 새 메모.
//!
//! 폴더 내용은 펼친 폴더만, 잠깐씩 캐시해 가며 읽어서 큰 폴더를 추가해도 가볍게 동작합니다.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId, Rect, Response, RichText, Sense, TextEdit, Ui, pos2, vec2};

use crate::folder_meta::{self, FolderMeta, rel_key};
use crate::settings::{SidebarSort, SortKey, SortOrder, mix};
use crate::ui_kit::{self, Colors, Icon};

pub const NOTE_EXTENSIONS: &[&str] = &["md", "markdown", "txt"];
pub const SKIP_DIRS: &[&str] = &["node_modules", "target", "$RECYCLE.BIN", "System Volume Information"];
const CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    modified: Option<std::time::SystemTime>,
    created: Option<std::time::SystemTime>,
}

impl SortOrder {
    fn compare(self, a: &Entry, b: &Entry) -> std::cmp::Ordering {
        let by_key = match self.key {
            SortKey::Name => natural_cmp(&a.name, &b.name),
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Created => a.created.cmp(&b.created),
        };
        let by_key = if self.descending { by_key.reverse() } else { by_key };
        // 날짜가 같으면 이름 순
        by_key.then_with(|| natural_cmp(&a.name, &b.name))
    }
}

/// 폴더가 먼저, 폴더끼리는 `sort.folders`, 메모끼리는 `sort.notes` 순서로
fn sort_entries(entries: &mut [Entry], sort: SidebarSort) {
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| if a.is_dir { sort.folders } else { sort.notes }.compare(a, b))
    });
}

/// 사이드바에서 일어난 일 (앱이 처리)
pub enum Action {
    Open(PathBuf),
    NewNote(PathBuf),
    NewFolder(PathBuf),
    NewQuickNote,
    AddRoot,
    RemoveRoot(PathBuf),
    Rename {
        from: PathBuf,
        to_name: String,
    },
    Delete(PathBuf),
    Reveal(PathBuf),
    /// 이 파일·폴더를 가리키는 링크를 클립보드로
    CopyLink(PathBuf),
    /// 이전 버전 창
    History(PathBuf),
    /// 끌어다 놓아 옮기기: 이 파일·폴더를 `to_dir` 안으로
    Move {
        from: PathBuf,
        to_dir: PathBuf,
    },
    ActivateDoc(u64),
    CloseDoc(u64),
}

/// 폴더 밖에서 연 문서나 저장 안 한 새 문서
pub struct LooseDoc {
    pub id: u64,
    pub title: String,
    pub dirty: bool,
    pub active: bool,
}

#[derive(Default)]
pub struct Sidebar {
    pub query: String,
    /// 접어 둔 최상위 폴더 (최상위는 기본으로 펼침)
    collapsed_roots: HashSet<PathBuf>,
    /// 펼친 하위 폴더
    expanded: HashSet<PathBuf>,
    cache: HashMap<PathBuf, (Instant, Vec<Entry>)>,
    renaming: Option<(PathBuf, String, bool)>,
    search: Option<(String, Instant, Found)>,
    /// 뒤에서 도는 검색 (검색어, 결과 받을 곳). 한 번에 하나만 돌려서 빠르게 쳐도 디스크를 덜 씀
    search_job: Option<(String, std::sync::mpsc::Receiver<Found>)>,
    /// 링크로 찾아온 폴더: 잠시 강조하고 한 번 그 자리로 스크롤
    flash: Option<(PathBuf, Instant, bool)>,
    /// 지금 정렬 (설정에서 받아 옴)
    sort: SidebarSort,
    /// 정렬 드롭다운을 펼쳐 둔 메모 폴더
    sort_open: Option<PathBuf>,
    /// 정렬을 한 번이라도 받았는지 (바뀌면 끌어서 정한 순서를 지움)
    sort_seen: bool,
    /// 메모 폴더마다 고정 · 숨김 · 순서 (읽은 때와 함께)
    meta: HashMap<PathBuf, (Instant, FolderMeta)>,
    /// 숨긴 메모 · 폴더도 보이기
    show_hidden: bool,
}

/// 검색 결과
#[derive(Clone, Default)]
struct Found {
    notes: Vec<PathBuf>,
    /// 태그 검색일 때: 치고 있는 태그로 시작하는 태그들 (태그, 메모 수)
    tags: Vec<(String, usize)>,
    /// 태그 검색식이었는지
    tag_mode: bool,
}

pub fn is_note(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| NOTE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

fn display_name(path: &Path, is_dir: bool) -> String {
    let os = if is_dir { path.file_name() } else { path.file_stem() };
    os.map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

impl Sidebar {
    /// 파일을 만들거나 지운 뒤 목록을 바로 다시 읽게 합니다.
    pub fn invalidate(&mut self) {
        self.cache.clear();
        self.search = None;
        self.meta.clear();
    }

    /// 메모 폴더의 고정 · 숨김 · 순서 (몇 초마다 파일에서 다시 읽음: 다른 PC 에서 바꾼 것도 반영)
    fn meta_of(&mut self, root: &Path) -> &mut FolderMeta {
        let stale = self.meta.get(root).is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(5));
        if stale {
            self.meta.insert(root.to_path_buf(), (Instant::now(), FolderMeta::load(root)));
        }
        &mut self.meta.get_mut(root).expect("방금 넣음").1
    }

    fn save_meta(&mut self, root: &Path) {
        if let Some((t, m)) = self.meta.get_mut(root) {
            m.save(root);
            *t = Instant::now();
        }
    }

    /// 사이드바 밖에서 옮기거나 이름을 바꿈: 고정 · 숨김 · 순서가 따라갑니다
    pub fn path_moved(&mut self, roots: &[PathBuf], from: &Path, to: &Path) {
        let (Some(fr), Some(tr)) =
            (folder_meta::root_of(roots, from).cloned(), folder_meta::root_of(roots, to).cloned())
        else {
            return;
        };
        let (Some(fk), Some(tk)) = (rel_key(&fr, from), rel_key(&tr, to)) else { return };
        if fr == tr {
            self.meta_of(&fr).moved(&fk, &tk);
        } else {
            let old = self.meta_of(&fr).clone();
            self.meta_of(&tr).adopt(&old, &fk, &tk);
            self.meta_of(&fr).removed(&fk);
            self.save_meta(&tr);
        }
        self.save_meta(&fr);
    }

    /// 휴지통으로 옮김
    pub fn path_removed(&mut self, roots: &[PathBuf], path: &Path) {
        let Some(root) = folder_meta::root_of(roots, path).cloned() else { return };
        let Some(key) = rel_key(&root, path) else { return };
        self.meta_of(&root).removed(&key);
        self.save_meta(&root);
    }

    fn is_hidden_path(&mut self, roots: &[PathBuf], path: &Path) -> bool {
        let Some(root) = folder_meta::root_of(roots, path).cloned() else { return false };
        let key = rel_key(&root, path).unwrap_or_default();
        self.meta_of(&root).is_hidden(&key)
    }

    /// 이 파일이 보이도록 상위 폴더들을 펼칩니다.
    pub fn reveal(&mut self, path: &Path, roots: &[PathBuf]) {
        for root in roots {
            if let Ok(rel) = path.strip_prefix(root) {
                self.collapsed_roots.remove(root);
                let mut dir = root.clone();
                let comps: Vec<_> = rel.components().collect();
                for c in comps.iter().take(comps.len().saturating_sub(1)) {
                    dir.push(c);
                    self.expanded.insert(dir.clone());
                }
            }
        }
    }

    /// 이 태그가 붙은 메모를 모아 봅니다 (본문의 태그를 Ctrl+클릭).
    pub fn show_tag(&mut self, tag: &str) {
        self.query = format!("#{tag}");
    }

    /// 링크로 찾아온 폴더를 펼쳐서 보여 주고 잠시 강조합니다.
    pub fn focus_folder(&mut self, dir: &Path, roots: &[PathBuf]) {
        self.query.clear();
        self.reveal(dir, roots);
        if roots.iter().any(|r| r == dir) {
            self.collapsed_roots.remove(dir);
        } else {
            self.expanded.insert(dir.to_path_buf());
        }
        self.flash = Some((dir.to_path_buf(), Instant::now(), true));
    }

    pub fn start_rename(&mut self, path: &Path) {
        let name = display_name(path, path.is_dir());
        self.renaming = Some((path.to_path_buf(), name, true));
    }

    fn entries(&mut self, dir: &Path, skip_name: &str) -> Vec<Entry> {
        if let Some((t, e)) = self.cache.get(dir)
            && t.elapsed() < CACHE_TTL
        {
            return e.clone();
        }
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || name.starts_with('~') {
                    continue;
                }
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                if is_dir {
                    if SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) || name.eq_ignore_ascii_case(skip_name) {
                        continue;
                    }
                } else if !is_note(&path) {
                    continue;
                }
                let meta = entry.metadata().ok();
                let modified = meta.as_ref().and_then(|m| m.modified().ok());
                let created = meta.as_ref().and_then(|m| m.created().ok());
                out.push(Entry { name: display_name(&path, is_dir), path, is_dir, modified, created });
            }
        }
        self.cache.insert(dir.to_path_buf(), (Instant::now(), out.clone()));
        out
    }

    /// 폴더 내용을 보일 순서대로: 정렬 → 끌어서 정한 순서 → 고정한 것을 맨 위로. 숨긴 것은 빼거나 (보이기면) 둠
    fn sorted_entries(&mut self, root: &Path, dir: &Path, skip_name: &str) -> Vec<Entry> {
        let mut entries = self.entries(dir, skip_name);
        sort_entries(&mut entries, self.sort);
        let show_hidden = self.show_hidden;
        let meta = self.meta_of(root).clone();
        let key = |e: &Entry| rel_key(root, &e.path).unwrap_or_default();
        if !show_hidden {
            entries.retain(|e| !meta.is_hidden(&key(e)));
        }
        if let Some(order) = meta.order.get(&rel_key(root, dir).unwrap_or_default()) {
            let pos = |e: &Entry| order.iter().position(|n| *n == file_name(&e.path)).unwrap_or(usize::MAX);
            // 폴더가 위인 것은 그대로, 정한 순서에 없는 것은 정렬 순서대로 뒤에 (안정 정렬)
            entries.sort_by_key(|e| (!e.is_dir, pos(e)));
        }
        entries.sort_by_key(|e| meta.pinned.iter().position(|p| *p == key(e)).unwrap_or(usize::MAX));
        entries
    }

    /// 같은 폴더 안에서 끌어 놓아 순서를 바꿈: `item` 을 `target` 앞(`before`)이나 뒤로.
    /// 둘 다 고정한 것이면 고정 순서를, 아니면 이 폴더의 순서를 바꿉니다.
    fn reorder(&mut self, root: &Path, dir: &Path, shown: &[PathBuf], item: &Path, target: &Path, before: bool) {
        let (Some(ik), Some(tk), Some(dk)) = (rel_key(root, item), rel_key(root, target), rel_key(root, dir)) else {
            return;
        };
        let meta = self.meta_of(root);
        if meta.is_pinned(&ik) && meta.is_pinned(&tk) {
            meta.pinned.retain(|k| *k != ik);
            let at = meta.pinned.iter().position(|k| *k == tk).unwrap_or(0) + usize::from(!before);
            meta.pinned.insert(at.min(meta.pinned.len()), ik);
        } else {
            let mut names: Vec<String> = shown.iter().map(|p| file_name(p)).filter(|n| *n != file_name(item)).collect();
            let at =
                names.iter().position(|n| *n == file_name(target)).map_or(names.len(), |i| i + usize::from(!before));
            names.insert(at, file_name(item));
            meta.set_order(&dk, names);
        }
        self.save_meta(root);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        roots: &[PathBuf],
        active: Option<&Path>,
        loose: &[LooseDoc],
        image_folder: &str,
        sort: &mut SidebarSort,
        show_hidden: &mut bool,
    ) -> Vec<Action> {
        let k = ui_kit::colors(ui);
        let mut actions = Vec::new();
        // 정렬을 바꾸면 끌어서 정한 순서는 지움 (고정은 그대로)
        if self.sort_seen && self.sort != *sort {
            for root in roots {
                if !self.meta_of(root).order.is_empty() {
                    self.meta_of(root).order.clear();
                    self.save_meta(root);
                }
            }
        }
        self.sort = *sort;
        self.sort_seen = true;
        self.show_hidden = *show_hidden;

        // 검색 칸
        search_box(ui, &mut self.query, k);
        ui.add_space(6.0);
        if action_row(ui, Icon::Plus, t!("새 메모"), "Ctrl+N", k).clicked() {
            actions.push(Action::NewQuickNote);
        }
        if action_row(ui, Icon::Folder, t!("폴더 추가"), "", k).clicked() {
            actions.push(Action::AddRoot);
        }
        if action_row(ui, Icon::Tag, t!("태그 모아보기"), "", k).clicked() {
            self.query = if self.query.trim_start().starts_with('#') { String::new() } else { "#".into() };
        }
        ui.add_space(10.0);

        egui::ScrollArea::vertical().id_salt("sidebar_tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if !self.query.trim().is_empty() {
                self.search_results(ui, roots, active, &mut actions, k);
                return;
            }

            if !loose.is_empty() {
                section_label(ui, t!("열린 문서"), k);
                for d in loose {
                    let label = if d.title.is_empty() { t!("제목 없음").to_owned() } else { d.title.clone() };
                    let mark = RowMark { dirty: d.dirty, ..Default::default() };
                    let resp = tree_row(ui, 0, RowIcon::Note, &label, d.active, mark, k);
                    let close = hover_icon(ui, &resp, Icon::Close, t!("닫기"), k);
                    if close {
                        actions.push(Action::CloseDoc(d.id));
                    } else if resp.clicked() {
                        actions.push(Action::ActivateDoc(d.id));
                    }
                }
                ui.add_space(12.0);
            }

            for root in roots {
                let collapsed = self.collapsed_roots.contains(root);
                let name = display_name(root, true);
                let name = if name.is_empty() { root.display().to_string() } else { name };
                let header = root_header(ui, &name, collapsed, k);
                self.scroll_to_flash(root, &header);
                drop_target(ui, &header, root, &mut actions, k);
                let tip = root.display().to_string();
                let header = header.on_hover_text(tip);
                let plus = hover_icon_always(ui, &header, Icon::Plus, t!("이 폴더에 새 메모"), k);
                // 정렬 (폴더 · 메모 따로, 모든 메모 폴더에 같이 적용)
                let sort_btn = header_icon(ui, &header, Icon::Sort, t!("정렬"), k, 42.0);
                if sort_btn.clicked() {
                    self.sort_open = if self.sort_open.as_ref() == Some(root) { None } else { Some(root.clone()) };
                }
                let mut open = self.sort_open.as_ref() == Some(root);
                if open {
                    // 열림 상태를 직접 들고 있어서, 안의 드롭다운을 열어도 이 창이 닫히지 않음
                    // (egui 가 기억하는 팝업은 한 번에 하나뿐이라 드롭다운이 열리면 바깥 메뉴가 밀려남)
                    let shown = egui::Popup::menu(&sort_btn)
                        .open_bool(&mut open)
                        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
                        .gap(4.0)
                        .show(|ui| sort_menu(ui, sort));
                    // 바깥을 누르면 닫음. 드롭다운 목록이 열려 있으면 그 클릭은 목록이 받으므로 그대로 둠
                    let popup = shown.map(|r| r.response.rect);
                    let outside = ui.input(|i| {
                        i.pointer.any_click()
                            && i.pointer
                                .interact_pos()
                                .is_some_and(|p| !popup.is_some_and(|r| r.contains(p)) && !sort_btn.rect.contains(p))
                    });
                    if outside && !egui::Popup::is_any_open(ui.ctx()) {
                        open = false;
                    }
                    if !open {
                        self.sort_open = None;
                    }
                }
                if plus {
                    actions.push(Action::NewNote(root.clone()));
                } else if header.clicked() && !sort_btn.clicked() {
                    if collapsed {
                        self.collapsed_roots.remove(root);
                    } else {
                        self.collapsed_roots.insert(root.clone());
                    }
                }
                header.context_menu(|ui| {
                    if ui.checkbox(show_hidden, t!("숨긴 메모 · 폴더 보이기")).clicked() {
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("새 메모")).clicked() {
                        actions.push(Action::NewNote(root.clone()));
                        ui.close();
                    }
                    if ui.button(t!("새 폴더")).clicked() {
                        actions.push(Action::NewFolder(root.clone()));
                        ui.close();
                    }
                    if ui.button(t!("탐색기에서 열기")).clicked() {
                        actions.push(Action::Reveal(root.clone()));
                        ui.close();
                    }
                    if ui.button(t!("하이퍼링크 복사")).clicked() {
                        actions.push(Action::CopyLink(root.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("목록에서 빼기 (파일은 그대로)")).clicked() {
                        actions.push(Action::RemoveRoot(root.clone()));
                        ui.close();
                    }
                });
                if !collapsed {
                    if !root.exists() {
                        ui.label(RichText::new(t!("    폴더를 찾을 수 없습니다")).small().weak());
                    } else {
                        self.tree(ui, root, root, 0, active, image_folder, &mut actions, k);
                    }
                }
                ui.add_space(10.0);
            }
            if roots.is_empty() {
                ui.label(RichText::new(t!("'폴더 추가'로 메모를 모아 둘 폴더를 고르세요.")).small().weak());
            }
            // 빈 곳을 오른쪽 클릭: 숨긴 것 보이기
            let rest = vec2(ui.available_width(), ui.available_height().max(80.0));
            let (_, blank) = ui.allocate_exact_size(rest, Sense::click());
            blank.context_menu(|ui| {
                if ui.checkbox(show_hidden, t!("숨긴 메모 · 폴더 보이기")).clicked() {
                    ui.close();
                }
            });
            drag_ghost(ui, k);
        });
        actions
    }

    #[allow(clippy::too_many_arguments)]
    fn tree(
        &mut self,
        ui: &mut Ui,
        root: &Path,
        dir: &Path,
        depth: usize,
        active: Option<&Path>,
        image_folder: &str,
        actions: &mut Vec<Action>,
        k: Colors,
    ) {
        let entries = self.sorted_entries(root, dir, image_folder);
        if entries.is_empty() && depth == 0 {
            ui.label(RichText::new(t!("    아직 메모가 없습니다")).small().weak());
        }
        let shown: Vec<PathBuf> = entries.iter().map(|e| e.path.clone()).collect();
        // (옮길 것, 기준, 앞에) · (경로, 고정이면 true · 숨김이면 false, 켬)
        let mut reorder: Option<(PathBuf, PathBuf, bool)> = None;
        let mut flag: Option<(String, bool, bool)> = None;
        for e in entries {
            let key = rel_key(root, &e.path).unwrap_or_default();
            let (pinned, hidden) = {
                let m = self.meta_of(root);
                (m.is_pinned(&key), m.hidden.contains(&key))
            };
            let is_active = active.is_some_and(|a| a == e.path);
            let open = e.is_dir && self.expanded.contains(&e.path);

            // 이름 바꾸는 중
            if let Some((path, text, focus)) = &mut self.renaming
                && *path == e.path
            {
                let mut done = None;
                ui.horizontal(|ui| {
                    ui.add_space(10.0 + depth as f32 * 14.0 + 22.0);
                    let resp = ui.add(
                        TextEdit::singleline(text)
                            .desired_width(ui.available_width() - 8.0)
                            .margin(egui::Margin::symmetric(6, 3)),
                    );
                    if *focus {
                        resp.request_focus();
                        *focus = false;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = Some(None);
                    } else if resp.lost_focus() {
                        done = Some(Some(text.trim().to_owned()));
                    }
                });
                if let Some(result) = done {
                    if let Some(name) = result.filter(|n| !n.is_empty() && *n != e.name) {
                        actions.push(Action::Rename { from: e.path.clone(), to_name: name });
                    }
                    self.renaming = None;
                }
                continue;
            }

            let icon = if e.is_dir { RowIcon::Folder { open } } else { RowIcon::Note };
            let flashing = self.flashing(&e.path);
            let mark = RowMark { dirty: false, pinned, dim: hidden };
            let resp = tree_row(ui, depth, icon, &e.name, is_active || flashing, mark, k);
            self.scroll_to_flash(&e.path, &resp);
            resp.dnd_set_drag_payload(e.path.clone());
            // 같은 폴더의 것을 위 · 아래 가장자리에 놓으면 순서 바꾸기 (폴더는 가운데에 놓으면 그 안으로 옮기기)
            let zone = resp
                .dnd_hover_payload::<PathBuf>()
                .filter(|from| from.parent() == Some(dir) && **from != e.path)
                .and_then(|_| ui.input(|i| i.pointer.hover_pos()))
                .and_then(|p| {
                    let t = (p.y - resp.rect.top()) / resp.rect.height();
                    if !e.is_dir {
                        Some(t < 0.5)
                    } else if t < 0.3 {
                        Some(true)
                    } else if t > 0.7 {
                        Some(false)
                    } else {
                        None
                    }
                });
            if let Some(before) = zone {
                let y = if before { resp.rect.top() } else { resp.rect.bottom() };
                ui.painter().line_segment(
                    [pos2(resp.rect.left() + 8.0, y), pos2(resp.rect.right() - 8.0, y)],
                    egui::Stroke::new(2.0, k.accent),
                );
                if let Some(from) = resp.dnd_release_payload::<PathBuf>() {
                    reorder = Some(((*from).clone(), e.path.clone(), before));
                }
            } else {
                // 끌어서 다른 폴더로 옮기기: 폴더에 놓으면 그 안으로, 메모에 놓으면 그 메모가 있는 폴더로
                let drop_dir = if e.is_dir { e.path.clone() } else { dir.to_path_buf() };
                drop_target(ui, &resp, &drop_dir, actions, k);
            }
            if e.is_dir {
                let plus = hover_icon(ui, &resp, Icon::Plus, t!("이 폴더에 새 메모"), k);
                if plus {
                    self.expanded.insert(e.path.clone());
                    actions.push(Action::NewNote(e.path.clone()));
                } else if resp.clicked() {
                    if open {
                        self.expanded.remove(&e.path);
                    } else {
                        self.expanded.insert(e.path.clone());
                    }
                }
            } else if resp.clicked() {
                actions.push(Action::Open(e.path.clone()));
            }
            if resp.double_clicked() && !e.is_dir {
                self.start_rename(&e.path);
            }
            resp.context_menu(|ui| {
                if e.is_dir {
                    if ui.button(t!("새 메모")).clicked() {
                        self.expanded.insert(e.path.clone());
                        actions.push(Action::NewNote(e.path.clone()));
                        ui.close();
                    }
                    if ui.button(t!("새 폴더")).clicked() {
                        self.expanded.insert(e.path.clone());
                        actions.push(Action::NewFolder(e.path.clone()));
                        ui.close();
                    }
                    ui.separator();
                } else if ui.button(t!("열기")).clicked() {
                    actions.push(Action::Open(e.path.clone()));
                    ui.close();
                }
                if ui.button(t!("이름 바꾸기")).clicked() {
                    self.start_rename(&e.path);
                    ui.close();
                }
                if ui.button(t!("탐색기에서 보기")).clicked() {
                    actions.push(Action::Reveal(e.path.clone()));
                    ui.close();
                }
                if ui.button(t!("하이퍼링크 복사")).clicked() {
                    actions.push(Action::CopyLink(e.path.clone()));
                    ui.close();
                }
                if !e.is_dir && ui.button(t!("이전 버전…")).clicked() {
                    actions.push(Action::History(e.path.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button(if pinned { t!("고정 풀기") } else { t!("상단에 고정") }).clicked() {
                    flag = Some((key.clone(), true, !pinned));
                    ui.close();
                }
                if ui.button(if hidden { t!("숨김 풀기") } else { t!("숨기기") }).clicked() {
                    flag = Some((key.clone(), false, !hidden));
                    ui.close();
                }
                ui.separator();
                if ui.button(t!("휴지통으로 옮기기")).clicked() {
                    actions.push(Action::Delete(e.path.clone()));
                    ui.close();
                }
            });
            if open {
                self.tree(ui, root, &e.path, depth + 1, active, image_folder, actions, k);
            }
        }
        if let Some((item, target, before)) = reorder {
            self.reorder(root, dir, &shown, &item, &target, before);
        }
        if let Some((key, is_pin, on)) = flag {
            let m = self.meta_of(root);
            if is_pin {
                m.set_pinned(&key, on)
            } else {
                m.set_hidden(&key, on)
            }
            self.save_meta(root);
        }
    }

    fn flashing(&self, path: &Path) -> bool {
        self.flash.as_ref().is_some_and(|(p, t, _)| p == path && t.elapsed() < Duration::from_millis(2500))
    }

    fn scroll_to_flash(&mut self, path: &Path, resp: &Response) {
        if let Some((p, t, scroll)) = &mut self.flash
            && p == path
        {
            if *scroll {
                resp.scroll_to_me(Some(egui::Align::Center));
                *scroll = false;
            }
            // 강조가 끝날 때 한 번 더 그려서 지움
            resp.ctx.request_repaint_after(Duration::from_millis(2500).saturating_sub(t.elapsed()));
        }
    }

    fn search_results(
        &mut self,
        ui: &mut Ui,
        roots: &[PathBuf],
        active: Option<&Path>,
        actions: &mut Vec<Action>,
        k: Colors,
    ) {
        let q = self.query.trim().to_lowercase();
        // 뒤에서 끝난 검색 결과 받기
        if let Some((jq, rx)) = &self.search_job {
            match rx.try_recv() {
                Ok(found) => {
                    self.search = Some((jq.clone(), Instant::now(), found));
                    self.search_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.search_job = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        // 검색어가 바뀌었거나 결과가 3초 넘게 지났으면 다시 (이미 돌고 있으면 그게 끝난 뒤에)
        let stale = self.search.as_ref().is_none_or(|(sq, t, _)| *sq != q || t.elapsed() > Duration::from_secs(3));
        if stale && self.search_job.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let (roots, query, ctx) = (roots.to_vec(), q.clone(), ui.ctx().clone());
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let found = search_notes(&roots, &query);
                crate::perf::record("search", started.elapsed());
                crate::perf::set_note("search_last", format!("'{}' → {}개", query, found.notes.len()));
                let _ = tx.send(found);
                ctx.request_repaint();
            });
            self.search_job = Some((q.clone(), rx));
        }
        let searching = self.search_job.is_some() && self.search.as_ref().is_none_or(|(sq, _, _)| *sq != q);
        if searching {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let found = self.search.as_ref().map(|(_, _, r)| r.clone()).unwrap_or_default();
        // 태그 검색: 치고 있는 태그로 시작하는 태그들 (누르면 그 태그로 채움)
        if !found.tags.is_empty() {
            section_label(ui, t!("태그"), k);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                ui.add_space(8.0);
                for (tag, count) in &found.tags {
                    let chip = egui::Button::new(RichText::new(format!("#{tag}  {count}")).size(12.5).color(k.accent))
                        .fill(mix(k.panel, k.accent, 0.10))
                        .corner_radius(10);
                    if ui.add(chip).clicked() {
                        self.query = crate::tags::complete_last(&self.query, tag);
                        ui.memory_mut(|m| m.request_focus(egui::Id::new("sidebar_search")));
                    }
                }
            });
            ui.add_space(6.0);
        }
        let results = found.notes;
        let tag_query_empty = found.tag_mode && crate::tags::TagQuery::parse(&q).is_none_or(|t| t.is_empty());
        if searching {
            section_label(ui, t!("찾는 중…"), k);
        } else if tag_query_empty {
            if found.tags.is_empty() {
                hint_label(ui, t!("아직 태그가 없습니다. 글에 #태그 처럼 쓰면 됩니다."), k);
            }
            hint_label(ui, t!("#a #b 는 둘 다, #a OR #b 는 둘 중 하나, -#a 는 빼기"), k);
        } else if found.tag_mode {
            section_label(ui, &tf!("태그가 맞는 메모 {}개", results.len()), k);
        } else {
            section_label(ui, &tf!("검색 결과 {}개", results.len()), k);
        }
        for p in results {
            // 숨긴 메모는 검색에도 (보이기를 켜지 않았으면) 나오지 않음
            let hidden = self.is_hidden_path(roots, &p);
            if hidden && !self.show_hidden {
                continue;
            }
            let name = display_name(&p, false);
            let mark = RowMark { dim: hidden, ..Default::default() };
            let resp = tree_row(ui, 0, RowIcon::Note, &name, active == Some(p.as_path()), mark, k);
            let folder = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
            let resp = resp.on_hover_text(folder);
            if resp.clicked() {
                actions.push(Action::Open(p.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button(t!("열기")).clicked() {
                    actions.push(Action::Open(p.clone()));
                    ui.close();
                }
                if ui.button(t!("하이퍼링크 복사")).clicked() {
                    actions.push(Action::CopyLink(p.clone()));
                    ui.close();
                }
                if ui.button(t!("탐색기에서 보기")).clicked() {
                    actions.push(Action::Reveal(p.clone()));
                    ui.close();
                }
            });
        }
    }
}

/// 검색어가 태그 검색식(`#a #b`, `#a OR #b` …)이면 태그로, 아니면 이름·내용으로 찾습니다.
/// 화면을 멈추지 않도록 뒤의 스레드에서 부릅니다.
fn search_notes(roots: &[PathBuf], query: &str) -> Found {
    match crate::tags::TagQuery::parse(query) {
        Some(tq) => search_tags(roots, &tq),
        None => Found { notes: search_text(roots, query), ..Default::default() },
    }
}

/// 메모 내용 (열 때와 똑같이 인코딩을 알아내 읽음, 너무 큰 파일은 건너뜀)
fn note_text(p: &Path) -> Option<String> {
    if std::fs::metadata(p).ok()?.len() >= 512 * 1024 {
        return None;
    }
    Some(crate::encoding::decode(&std::fs::read(p).ok()?).0)
}

/// 태그 검색: 검색식에 맞는 메모(이름 순, 최대 500개)와, 치고 있는 태그로 시작하는 태그(많이 쓴 순, 최대 40개)
fn search_tags(roots: &[PathBuf], tq: &crate::tags::TagQuery) -> Found {
    let mut notes = Vec::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut scanned = 0usize;
    for root in roots {
        walk(root, 0, &mut |p| {
            scanned += 1;
            if scanned > 20_000 || !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
                return;
            }
            let Some(text) = note_text(p) else { return };
            let tags = crate::tags::extract(&text);
            for t in &tags {
                *counts.entry(t.clone()).or_default() += 1;
            }
            if tq.matches(&tags) {
                notes.push(p.to_path_buf());
            }
        });
    }
    notes.sort_by(|a, b| natural_cmp(&display_name(a, false), &display_name(b, false)));
    notes.truncate(500);
    let mut tags: Vec<(String, usize)> = match &tq.typing {
        Some(prefix) => counts.into_iter().filter(|(t, _)| t.starts_with(prefix.as_str())).collect(),
        None => Vec::new(),
    };
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    tags.truncate(40);
    Found { notes, tags, tag_mode: true }
}

/// 메모 폴더들에서 이름 또는 내용에 `query`(소문자)가 들어간 메모. 이름이 맞는 것부터, 최대 200개.
fn search_text(roots: &[PathBuf], query: &str) -> Vec<PathBuf> {
    let mut by_name = Vec::new();
    let mut by_content = Vec::new();
    let mut scanned = 0usize;
    for root in roots {
        walk(root, 0, &mut |p| {
            scanned += 1;
            if scanned > 20_000 || by_name.len() + by_content.len() >= 200 {
                return;
            }
            if display_name(p, false).to_lowercase().contains(query) {
                by_name.push(p.to_path_buf());
            } else if note_text(p).is_some_and(|t| t.to_lowercase().contains(query)) {
                by_content.push(p.to_path_buf());
            }
        });
    }
    by_name.extend(by_content);
    by_name.truncate(200);
    by_name
}

/// 폴더 안의 하위 폴더와 메모 파일을 모두 훑습니다 (사이드바에 보이는 것과 같은 것만, 깊이·개수 제한)
pub fn walk_tree(dir: &Path, skip_name: &str, depth: usize, f: &mut dyn FnMut(&Path, bool)) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten().take(3000) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with('~') {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if !SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) && !name.eq_ignore_ascii_case(skip_name) {
                f(&path, true);
                walk_tree(&path, skip_name, depth + 1, f);
            }
        } else if is_note(&path) {
            f(&path, false);
        }
    }
}

/// 폴더 안의 메모 파일을 모두 훑습니다 (깊이·개수 제한)
fn walk(dir: &Path, depth: usize, f: &mut dyn FnMut(&Path)) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut count = 0;
    for entry in rd.flatten() {
        count += 1;
        if count > 3000 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if !SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                walk(&path, depth + 1, f);
            }
        } else if is_note(&path) {
            f(&path);
        }
    }
}

/// "메모 2" < "메모 10" 처럼 숫자를 숫자로 비교
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let o = na.len().cmp(&nb.len()).then(na.cmp(&nb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 그리기 부품

enum RowIcon {
    Folder { open: bool },
    Note,
}

const ROW_H: f32 = 30.0;

/// 끌고 온 파일·폴더를 이 줄에 놓으면 `dir` 로 옮기기. 끌고 있는 동안 놓을 자리를 강조합니다.
fn drop_target(ui: &Ui, resp: &Response, dir: &Path, actions: &mut Vec<Action>, k: Colors) {
    if let Some(from) = resp.dnd_hover_payload::<PathBuf>()
        && !dir.starts_with(from.as_path())
    {
        ui.painter().rect_stroke(resp.rect, 8.0, egui::Stroke::new(1.5, k.accent), egui::StrokeKind::Inside);
    }
    if let Some(from) = resp.dnd_release_payload::<PathBuf>() {
        actions.push(Action::Move { from: (*from).clone(), to_dir: dir.to_path_buf() });
    }
}

/// 끌고 있는 파일·폴더 이름을 마우스 옆에 보여 줍니다.
fn drag_ghost(ui: &Ui, k: Colors) {
    let Some(path) = egui::DragAndDrop::payload::<PathBuf>(ui.ctx()) else { return };
    let Some(pos) = ui.ctx().pointer_interact_pos() else { return };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("sidebar_drag")));
    let galley = painter.layout_no_wrap(name, FontId::proportional(13.0), k.text);
    let rect = Rect::from_min_size(pos + vec2(14.0, 10.0), galley.size() + vec2(16.0, 8.0));
    painter.rect_filled(rect, 6.0, k.panel);
    painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, k.accent), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(8.0, 4.0), galley, k.text);
}

/// 줄 오른쪽 · 글자에 붙는 표시
#[derive(Clone, Copy, Default)]
struct RowMark {
    /// 저장 안 됨 (점)
    dirty: bool,
    /// 상단 고정 (핀)
    pinned: bool,
    /// 숨긴 것 (흐리게)
    dim: bool,
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn tree_row(ui: &mut Ui, depth: usize, icon: RowIcon, label: &str, active: bool, mark: RowMark, k: Colors) -> Response {
    // 누르기와 끌기(다른 폴더로 옮기기)를 함께 받음
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    let fill = if active {
        mix(k.panel, k.text, 0.09)
    } else if hovered {
        mix(k.panel, k.text, 0.05)
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, 8.0, fill);
    let x = rect.left() + 10.0 + depth as f32 * 14.0;
    let cy = rect.center().y;
    let icon_color = if active { k.text } else { k.muted };
    match icon {
        RowIcon::Folder { open } => {
            let chev = if open { Icon::ChevronDown } else { Icon::ChevronRight };
            ui_kit::paint_icon(
                p,
                Rect::from_center_size(pos2(x + 6.0, cy), vec2(11.0, 11.0)),
                chev,
                icon_color,
                k.panel,
            );
        }
        RowIcon::Note => {
            p.circle_stroke(pos2(x + 6.0, cy), 4.5, egui::Stroke::new(1.3, icon_color));
        }
    }
    let text_color = if active { k.text } else { mix(k.text, k.panel, 0.12) };
    let text_color = if mark.dim { mix(text_color, k.panel, 0.55) } else { text_color };
    let max_w = rect.right() - (x + 20.0) - 34.0;
    let galley = elided(ui, label, 13.5, max_w, text_color);
    ui.painter().galley(pos2(x + 20.0, cy - galley.size().y / 2.0), galley, text_color);
    if mark.dirty {
        ui.painter().circle_filled(pos2(rect.right() - 14.0, cy), 3.0, k.accent);
    } else if mark.pinned {
        // 핀: 머리 · 바늘
        let c = pos2(rect.right() - 14.0, cy - 2.0);
        ui.painter().circle_filled(c, 3.2, k.muted);
        ui.painter().line_segment([c, c + vec2(0.0, 7.0)], egui::Stroke::new(1.4, k.muted));
    }
    resp
}

fn elided(ui: &Ui, text: &str, size: f32, max_w: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(size), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(20.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

fn root_header(ui: &mut Ui, name: &str, collapsed: bool, k: Colors) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    let cy = rect.center().y;
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        p.rect_filled(rect, 8.0, mix(k.panel, k.text, 0.04));
    }
    ui_kit::paint_icon(
        p,
        Rect::from_center_size(pos2(rect.left() + 16.0, cy), vec2(15.0, 15.0)),
        Icon::Folder,
        k.muted,
        k.panel,
    );
    let galley = elided(ui, name, 12.5, rect.width() - 80.0, k.muted);
    ui.painter().galley(pos2(rect.left() + 30.0, cy - galley.size().y / 2.0), galley, k.muted);
    if hovered {
        let chev = if collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
        ui_kit::paint_icon(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 44.0, cy), vec2(10.0, 10.0)),
            chev,
            k.muted,
            k.panel,
        );
    }
    resp
}

/// 줄 오른쪽 끝에 마우스를 올렸을 때만 보이는 작은 아이콘 버튼. 눌렀으면 `true`
fn hover_icon(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors) -> bool {
    if !ui.rect_contains_pointer(row.rect) {
        return false;
    }
    small_icon_at(ui, row, icon, tip, k)
}

/// 정렬 창: 폴더 정렬, 메모 정렬 드롭다운
fn sort_menu(ui: &mut Ui, sort: &mut SidebarSort) {
    for (n, (title, order)) in
        [(t!("폴더 정렬"), &mut sort.folders), (t!("메모 정렬"), &mut sort.notes)].into_iter().enumerate()
    {
        ui.label(RichText::new(title).small().weak());
        egui::ComboBox::from_id_salt(("sidebar_sort", n)).selected_text(order.label()).width(230.0).show_ui(ui, |ui| {
            for o in SortOrder::ALL {
                ui.selectable_value(order, o, o.label());
            }
        });
        ui.add_space(6.0);
    }
}

/// 줄 오른쪽 끝에서 `dx` 만큼 안쪽에 늘 보이는 작은 아이콘 단추
fn header_icon(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors, dx: f32) -> Response {
    let r = Rect::from_center_size(pos2(row.rect.right() - dx, row.rect.center().y), vec2(22.0, 22.0));
    let resp = ui.interact(r, row.id.with(("header_icon", icon as u8)), Sense::click()).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, 6.0, mix(k.panel, k.text, 0.10));
    }
    let color = if resp.hovered() { k.text } else { k.muted };
    ui_kit::paint_icon(ui.painter(), r.shrink(5.0), icon, color, k.panel);
    resp
}

fn hover_icon_always(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors) -> bool {
    small_icon_at(ui, row, icon, tip, k)
}

fn small_icon_at(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors) -> bool {
    let r = Rect::from_center_size(pos2(row.rect.right() - 16.0, row.rect.center().y), vec2(22.0, 22.0));
    let resp = ui.interact(r, row.id.with(("hover_icon", icon as u8)), Sense::click()).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, 6.0, mix(k.panel, k.text, 0.10));
    }
    let color = if resp.hovered() { k.text } else { k.muted };
    ui_kit::paint_icon(ui.painter(), r.shrink(5.0), icon, color, k.panel);
    resp.clicked()
}

fn section_label(ui: &mut Ui, text: &str, k: Colors) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.0),
        k.muted,
    );
}

/// 여러 줄로 접히는 흐린 도움말
fn hint_label(ui: &mut Ui, text: &str, k: Colors) {
    ui.add_space(4.0);
    egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 8, top: 0, bottom: 4 }).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(k.muted)).wrap());
    });
}

fn action_row(ui: &mut Ui, icon: Icon, label: &str, shortcut: &str, k: Colors) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 8.0, mix(k.panel, k.text, 0.06));
    }
    let cy = rect.center().y;
    let c = pos2(rect.left() + 18.0, cy);
    p.circle_filled(c, 10.0, mix(k.panel, k.text, 0.08));
    ui_kit::paint_icon(p, Rect::from_center_size(c, vec2(11.0, 11.0)), icon, k.text, k.panel);
    p.text(pos2(rect.left() + 36.0, cy), Align2::LEFT_CENTER, label, FontId::proportional(14.0), k.text);
    if !shortcut.is_empty() && resp.hovered() {
        p.text(pos2(rect.right() - 10.0, cy), Align2::RIGHT_CENTER, shortcut, FontId::proportional(11.5), k.muted);
    }
    resp
}

fn search_box(ui: &mut Ui, query: &mut String, k: Colors) {
    let frame = egui::Frame::NONE
        .fill(mix(k.panel, k.text, 0.05))
        .stroke(egui::Stroke::new(1.0, mix(k.panel, k.text, 0.10)))
        .corner_radius(9)
        .inner_margin(egui::Margin { left: 30, right: 8, top: 5, bottom: 5 });
    let resp = frame.show(ui, |ui| {
        ui.add(
            TextEdit::singleline(query)
                .id(egui::Id::new("sidebar_search"))
                .hint_text(t!("검색  (Ctrl+P)"))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY),
        )
    });
    let r = resp.response.rect;
    ui_kit::paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(13.0, 13.0)),
        Icon::Search,
        k.muted,
        k.panel,
    );
    if !query.is_empty() {
        let x = Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(16.0, 16.0));
        let xr = ui.interact(x, resp.response.id.with("clear"), Sense::click());
        ui_kit::paint_icon(
            ui.painter(),
            x.shrink(3.0),
            Icon::Close,
            if xr.hovered() { k.text } else { k.muted },
            k.panel,
        );
        if xr.clicked() {
            query.clear();
        }
    }
}

/// 파일·폴더를 휴지통으로 (Windows). 다른 OS에서는 바로 지웁니다.
pub fn move_to_trash(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{
            FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW,
        };
        // 경로 목록은 \0\0 으로 끝나야 합니다.
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.extend([0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
            ..Default::default()
        };
        // SAFETY: op의 포인터들은 이 함수가 끝날 때까지 살아 있습니다.
        let rc = unsafe { SHFileOperationW(&mut op) };
        if rc != 0 || op.fAnyOperationsAborted != 0 {
            return Err(std::io::Error::other(tf!("휴지통으로 옮기지 못했습니다 (코드 {})", rc)));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
    }
}

/// 기본 메모 폴더: 문서\TB Notes
pub fn default_notes_dir() -> PathBuf {
    // 테스트 등으로 저장 폴더를 따로 지정했으면 그 안에 만듭니다.
    if let Some(dir) = std::env::var_os("TB_NOTEPAD_DATA") {
        return PathBuf::from(dir).join("TB Notes");
    }
    documents_dir().join("TB Notes")
}

fn documents_dir() -> PathBuf {
    #[cfg(windows)]
    {
        // "Shell Folders" 는 OneDrive 등으로 옮긴 실제 문서 폴더 경로를 담고 있습니다.
        if let Ok(key) = winreg::HKCU.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders")
            && let Ok(p) = key.get_value::<String, _>("Personal")
            && !p.is_empty()
        {
            return PathBuf::from(p);
        }
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Documents")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_folders_and_notes_separately() {
        use std::time::{Duration, SystemTime};
        let t = |s: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        let e = |name: &str, is_dir: bool, m: u64, c: u64| Entry {
            path: PathBuf::from(name),
            name: name.into(),
            is_dir,
            modified: t(m),
            created: t(c),
        };
        let mut list = vec![
            e("메모2", false, 30, 10),
            e("폴더B", true, 5, 50),
            e("메모10", false, 20, 30),
            e("폴더A", true, 9, 40),
            e("메모1", false, 10, 20),
        ];
        fn names(l: &[Entry]) -> Vec<&str> {
            l.iter().map(|e| e.name.as_str()).collect()
        }
        // 기본: 둘 다 이름 순 (숫자는 크기대로)
        sort_entries(&mut list, SidebarSort::default());
        assert_eq!(names(&list), ["폴더A", "폴더B", "메모1", "메모2", "메모10"]);
        // 폴더는 만든 날짜 최근 순, 메모는 수정한 날짜 최근 순
        let sort = SidebarSort {
            folders: SortOrder { key: SortKey::Created, descending: true },
            notes: SortOrder { key: SortKey::Modified, descending: true },
        };
        sort_entries(&mut list, sort);
        assert_eq!(names(&list), ["폴더B", "폴더A", "메모2", "메모10", "메모1"]);
        // 메모만 이름 역순, 폴더는 수정한 날짜 오래된 순
        let sort = SidebarSort {
            folders: SortOrder { key: SortKey::Modified, descending: false },
            notes: SortOrder { key: SortKey::Name, descending: true },
        };
        sort_entries(&mut list, sort);
        assert_eq!(names(&list), ["폴더B", "폴더A", "메모10", "메모2", "메모1"]);
    }

    #[test]
    fn tag_search_with_and_or() {
        let root = std::env::temp_dir().join(format!("tbn_tags_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("하위")).unwrap();
        std::fs::write(root.join("가.md"), "회의 #회의 #예산").unwrap();
        std::fs::write(root.join("하위/나.md"), "#회의 만").unwrap();
        std::fs::write(root.join("다.md"), "#여행 #예산").unwrap();
        std::fs::write(root.join("라.txt"), "#회의 (txt 는 태그 없음)").unwrap();
        let names = |q: &str| -> Vec<String> {
            search_notes(std::slice::from_ref(&root), q).notes.iter().map(|p| display_name(p, false)).collect()
        };
        assert_eq!(names("#회의"), ["가", "나"]);
        assert_eq!(names("#회의 #예산"), ["가"]);
        assert_eq!(names("#회의 or #여행"), ["가", "나", "다"]);
        assert_eq!(names("#예산 -#여행"), ["가"]);
        let found = search_notes(std::slice::from_ref(&root), "#");
        assert!(found.tag_mode && found.notes.is_empty());
        assert_eq!(found.tags[..2], [("예산".to_string(), 2), ("회의".to_string(), 2)]);
        let found = search_notes(std::slice::from_ref(&root), "#여");
        assert_eq!(found.tags, [("여행".to_string(), 1)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_reads_legacy_encodings() {
        let root = std::env::temp_dir().join(format!("tbn_search_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("회의록\r\n예산 검토는 다음 주에 합니다.\r\n");
        std::fs::write(root.join("옛날.txt"), &cp949).unwrap();
        let utf16: Vec<u8> =
            [0xFF, 0xFE].into_iter().chain("예산 메모".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(root.join("유니코드.txt"), utf16).unwrap();
        std::fs::write(root.join("다른 것.md"), "관계없는 글").unwrap();
        let mut found: Vec<_> = search_notes(std::slice::from_ref(&root), "예산")
            .notes
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, ["옛날.txt", "유니코드.txt"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn natural_sort() {
        let mut v = vec!["메모 10", "메모 2", "Apple", "메모 1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Apple", "메모 1", "메모 2", "메모 10"]);
    }
}
