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

mod fsutil;
mod search;
mod widgets;

pub use fsutil::*;
use search::*;
use widgets::*;

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
    /// 고정 · 숨김 · 순서를 저장하지 못한 이유 (앱이 상태 표시줄에 띄움)
    meta_error: Option<String>,
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

    /// 고정 · 숨김 · 순서를 바꿉니다. 저장 직전에 파일을 다시 읽고 이번 변경 하나만 반영해서,
    /// 그사이 다른 PC 에서 바꾼 것을 덮지 않게 합니다. 읽거나 쓰지 못하면 `meta_error` 에 남깁니다.
    fn update_meta(&mut self, root: &Path, change: impl FnOnce(&mut FolderMeta)) {
        match FolderMeta::try_load(root) {
            Ok(mut m) => {
                change(&mut m);
                if let Err(e) = m.save(root) {
                    self.meta_error = Some(e.to_string());
                }
                self.meta.insert(root.to_path_buf(), (Instant::now(), m));
            }
            Err(e) => self.meta_error = Some(e.to_string()),
        }
    }

    /// 저장하지 못한 이유 (한 번만)
    pub fn take_meta_error(&mut self) -> Option<String> {
        self.meta_error.take()
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
            self.update_meta(&fr, |m| m.moved(&fk, &tk));
        } else {
            let old = FolderMeta::load(&fr);
            self.update_meta(&tr, |m| m.adopt(&old, &fk, &tk));
            self.update_meta(&fr, |m| m.removed(&fk));
        }
    }

    /// 휴지통으로 옮김
    pub fn path_removed(&mut self, roots: &[PathBuf], path: &Path) {
        let Some(root) = folder_meta::root_of(roots, path).cloned() else { return };
        let Some(key) = rel_key(&root, path) else { return };
        self.update_meta(&root, |m| m.removed(&key));
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
        let mut names: Vec<String> = shown.iter().map(|p| file_name(p)).filter(|n| *n != file_name(item)).collect();
        let at = names.iter().position(|n| *n == file_name(target)).map_or(names.len(), |i| i + usize::from(!before));
        names.insert(at, file_name(item));
        self.update_meta(root, |meta| {
            if meta.is_pinned(&ik) && meta.is_pinned(&tk) {
                meta.pinned.retain(|k| *k != ik);
                let at = meta.pinned.iter().position(|k| *k == tk).unwrap_or(0) + usize::from(!before);
                meta.pinned.insert(at.min(meta.pinned.len()), ik);
            } else {
                meta.set_order(&dk, names);
            }
        });
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
                    self.update_meta(root, |m| m.order.clear());
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
            self.update_meta(root, |m| if is_pin { m.set_pinned(&key, on) } else { m.set_hidden(&key, on) });
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_changes_do_not_overwrite_other_pcs() {
        let root = std::env::temp_dir().join(format!("tbn_meta_merge_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut sb = Sidebar::default();
        sb.update_meta(&root, |m| m.set_pinned("가.md", true));
        // 다른 PC 가 그사이 "나.md" 를 고정 (이 PC 의 기억은 아직 옛것)
        let mut other = FolderMeta::load(&root);
        other.set_pinned("나.md", true);
        other.save(&root).unwrap();
        sb.update_meta(&root, |m| m.set_hidden("다", true));
        let saved = FolderMeta::load(&root);
        assert_eq!(saved.pinned, ["가.md", "나.md"], "다른 PC 의 변경이 남음");
        assert_eq!(saved.hidden, ["다"]);
        assert!(sb.take_meta_error().is_none());
        // 깨진 파일은 덮어쓰지 않고 오류로 알림
        std::fs::write(root.join(folder_meta::FILE), "{ 깨짐").unwrap();
        sb.update_meta(&root, |m| m.set_pinned("라.md", true));
        assert!(sb.take_meta_error().is_some());
        assert_eq!(std::fs::read_to_string(root.join(folder_meta::FILE)).unwrap(), "{ 깨짐");
        let _ = std::fs::remove_dir_all(&root);
    }

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
}
