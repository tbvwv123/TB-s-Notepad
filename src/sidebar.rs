//! 왼쪽 사이드바: 폴더 › 하위 폴더 › 메모 파일 트리, 검색, 새 메모.
//!
//! 폴더 내용은 펼친 폴더만, 잠깐씩 캐시해 가며 읽어서 큰 폴더를 추가해도 가볍게 동작합니다.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId, Rect, Response, RichText, Sense, TextEdit, Ui, pos2, vec2};

use crate::settings::mix;
use crate::ui_kit::{self, Colors, Icon};

pub const NOTE_EXTENSIONS: &[&str] = &["md", "markdown", "txt"];
pub const SKIP_DIRS: &[&str] = &["node_modules", "target", "$RECYCLE.BIN", "System Volume Information"];
const CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
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
    search: Option<(String, Instant, Vec<PathBuf>)>,
    /// 링크로 찾아온 폴더: 잠시 강조하고 한 번 그 자리로 스크롤
    flash: Option<(PathBuf, Instant, bool)>,
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
                out.push(Entry { name: display_name(&path, is_dir), path, is_dir });
            }
        }
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural_cmp(&a.name, &b.name)));
        self.cache.insert(dir.to_path_buf(), (Instant::now(), out.clone()));
        out
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        roots: &[PathBuf],
        active: Option<&Path>,
        loose: &[LooseDoc],
        image_folder: &str,
    ) -> Vec<Action> {
        let k = ui_kit::colors(ui);
        let mut actions = Vec::new();

        // 검색 칸
        search_box(ui, &mut self.query, k);
        ui.add_space(6.0);
        if action_row(ui, Icon::Plus, t!("새 메모"), "Ctrl+N", k).clicked() {
            actions.push(Action::NewQuickNote);
        }
        if action_row(ui, Icon::Folder, t!("폴더 추가"), "", k).clicked() {
            actions.push(Action::AddRoot);
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
                    let resp = tree_row(ui, 0, RowIcon::Note, &label, d.active, d.dirty, k);
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
                let tip = root.display().to_string();
                let header = header.on_hover_text(tip);
                let plus = hover_icon_always(ui, &header, Icon::Plus, t!("이 폴더에 새 메모"), k);
                if plus {
                    actions.push(Action::NewNote(root.clone()));
                } else if header.clicked() {
                    if collapsed {
                        self.collapsed_roots.remove(root);
                    } else {
                        self.collapsed_roots.insert(root.clone());
                    }
                }
                header.context_menu(|ui| {
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
                        self.tree(ui, root, 0, active, image_folder, &mut actions, k);
                    }
                }
                ui.add_space(10.0);
            }
            if roots.is_empty() {
                ui.label(RichText::new(t!("'폴더 추가'로 메모를 모아 둘 폴더를 고르세요.")).small().weak());
            }
        });
        actions
    }

    #[allow(clippy::too_many_arguments)]
    fn tree(
        &mut self,
        ui: &mut Ui,
        dir: &Path,
        depth: usize,
        active: Option<&Path>,
        image_folder: &str,
        actions: &mut Vec<Action>,
        k: Colors,
    ) {
        let entries = self.entries(dir, image_folder);
        if entries.is_empty() && depth == 0 {
            ui.label(RichText::new(t!("    아직 메모가 없습니다")).small().weak());
        }
        for e in entries {
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
            let resp = tree_row(ui, depth, icon, &e.name, is_active || flashing, false, k);
            self.scroll_to_flash(&e.path, &resp);
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
                ui.separator();
                if ui.button(t!("휴지통으로 옮기기")).clicked() {
                    actions.push(Action::Delete(e.path.clone()));
                    ui.close();
                }
            });
            if open {
                self.tree(ui, &e.path, depth + 1, active, image_folder, actions, k);
            }
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
        let stale = self.search.as_ref().is_none_or(|(sq, t, _)| *sq != q || t.elapsed() > Duration::from_secs(3));
        if stale {
            let mut found = Vec::new();
            let mut content_hits = Vec::new();
            let mut scanned = 0usize;
            for root in roots {
                walk(root, 0, &mut |p| {
                    scanned += 1;
                    let name = display_name(p, false).to_lowercase();
                    if name.contains(&q) {
                        found.push(p.to_path_buf());
                    } else if scanned < 600
                        && std::fs::metadata(p).is_ok_and(|m| m.len() < 512 * 1024)
                        && std::fs::read_to_string(p).is_ok_and(|t| t.to_lowercase().contains(&q))
                    {
                        content_hits.push(p.to_path_buf());
                    }
                });
            }
            found.extend(content_hits);
            found.truncate(200);
            self.search = Some((q.clone(), Instant::now(), found));
        }
        let results = self.search.as_ref().map(|(_, _, r)| r.clone()).unwrap_or_default();
        section_label(ui, &tf!("검색 결과 {}개", results.len()), k);
        for p in results {
            let name = display_name(&p, false);
            let resp = tree_row(ui, 0, RowIcon::Note, &name, active == Some(p.as_path()), false, k);
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

fn tree_row(ui: &mut Ui, depth: usize, icon: RowIcon, label: &str, active: bool, dirty: bool, k: Colors) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
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
    let max_w = rect.right() - (x + 20.0) - 34.0;
    let galley = elided(ui, label, 13.5, max_w, text_color);
    ui.painter().galley(pos2(x + 20.0, cy - galley.size().y / 2.0), galley, text_color);
    if dirty {
        ui.painter().circle_filled(pos2(rect.right() - 14.0, cy), 3.0, k.accent);
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
    fn natural_sort() {
        let mut v = vec!["메모 10", "메모 2", "Apple", "메모 1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Apple", "메모 1", "메모 2", "메모 10"]);
    }
}
