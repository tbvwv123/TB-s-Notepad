//! 메인 앱: 사이드바(폴더 트리), 문서 편집(서식이 바로 보이는 문서 모드 / 원본 보기),
//! 찾기, 설정, 자동 저장, 세션 복원.

use std::ops::Range;
use std::path::{Path, PathBuf};

use eframe::egui;
use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{
    Align, Align2, Color32, FontId, Frame, Id, Key, KeyboardShortcut, Margin, Modifiers, RichText, ScrollArea, Shape,
    Stroke, TextEdit, Ui, ViewportCommand,
};
use serde::{Deserialize, Serialize};

use crate::document::{DiskEvent, DiskStamp, Document, LineEnding, TextEncoding, remap_char_index};
use crate::fonts::{FontLibrary, bold_family, doc_family, editor_family};
use crate::live::{self, Deco, ImgAlign, LiveStyle};
use crate::markdown::{self, HighlightStyle, MdAction};
use crate::settings::{Palette, Settings, ThemePreset, ViewMode, c, mix};
use crate::sidebar::{self, Action, LooseDoc, Sidebar};
use crate::ui_kit::{self, Icon};

const APP_NAME: &str = "TB's Notepad";

const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "log", "ini", "cfg", "conf", "json", "csv", "tsv", "xml", "yaml", "yml", "toml", "html",
    "css", "js", "ts", "rs", "py", "c", "h", "cpp", "java", "go", "sh", "bat", "ps1",
];
const EDITOR_MARGIN: Margin = Margin { left: 18, right: 18, top: 22, bottom: 60 };
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];
/// 입력을 멈추고 이만큼 지나면 자동 저장
const AUTOSAVE_DELAY: f32 = 0.8;
/// 열린 문서의 파일이 밖에서 바뀌었는지 이 간격(초)으로 확인 (창이 포커스를 얻을 때도 바로 확인)
const DISK_CHECK_INTERVAL: f64 = 2.0;

#[derive(Serialize, Deserialize)]
struct SessionDoc {
    path: Option<PathBuf>,
    /// 저장하지 않은 내용 (저장된 파일은 다시 읽으므로 `None`)
    text: Option<String>,
    dirty: bool,
    encoding: TextEncoding,
    line_ending: LineEnding,
    untitled_index: usize,
    #[serde(default)]
    auto_named: bool,
    /// 저장하지 않은 내용을 복원할 때, 그 사이 파일이 밖에서 바뀌었는지 알아보려고
    #[serde(default)]
    disk: Option<DiskStamp>,
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    docs: Vec<SessionDoc>,
    active: usize,
}

/// 현재 화면에 적용된 모양 (바뀌었을 때만 다시 적용)
#[derive(Clone, PartialEq)]
struct Look {
    palette: Palette,
    editor_font: Option<String>,
    ui_font: Option<String>,
    doc_font: Option<String>,
    ui_scale: f32,
    always_on_top: bool,
}

enum Dialog {
    /// 저장하지 않은 문서 닫기 확인
    CloseDoc(u64),
    /// 저장하지 않은 문서가 있는 상태에서 종료
    Exit,
    Message(String),
    ConfirmDelete(PathBuf),
    NewFolder { parent: PathBuf, name: String },
    /// 앱에서 고친 사이 파일이 밖에서도 바뀜: 다시 불러오기 / 내 내용 유지
    ExternalChange(u64),
}

#[derive(Default)]
struct FindState {
    open: bool,
    replace: bool,
    query: String,
    replacement: String,
    match_case: bool,
    focus_query: bool,
    /// 현재 문서에서 찾은 위치들 (글자 단위)
    matches: Vec<Range<usize>>,
    matches_key: u64,
    /// 지금 가리키는 찾은 위치. 포커스가 없는 TextEdit은 선택 영역을 유지하지 않으므로
    /// 편집기 선택과 별도로 기억합니다.
    current: Option<Range<usize>>,
}

#[derive(Default, Clone, Copy)]
struct CursorInfo {
    line: usize,
    column: usize,
    selected: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    Theme,
    Font,
    Editor,
    Keys,
    About,
}

impl SettingsPage {
    const ALL: [SettingsPage; 5] = [SettingsPage::Theme, SettingsPage::Font, SettingsPage::Editor, SettingsPage::Keys, SettingsPage::About];

    fn label(self) -> &'static str {
        match self {
            SettingsPage::Theme => "테마와 색",
            SettingsPage::Font => "글꼴",
            SettingsPage::Editor => "편집기",
            SettingsPage::Keys => "단축키",
            SettingsPage::About => "정보",
        }
    }
}

pub struct NotepadApp {
    docs: Vec<Document>,
    active: usize,
    settings: Settings,
    fonts: FontLibrary,
    applied: Option<Look>,
    untitled_counter: usize,
    sidebar: Sidebar,

    find: FindState,
    dialog: Option<Dialog>,
    show_settings: bool,
    settings_page: SettingsPage,
    font_filter: String,

    pending_action: Option<MdAction>,
    pending_select: Option<Range<usize>>,
    /// 선택한 그림: (문서 id, 그림 줄이 시작하는 바이트 위치)
    selected_image: Option<(u64, usize)>,
    /// 지난 프레임의 커서 (그림 줄에 들어올 때 어느 쪽에서 왔는지 알려고)
    last_cursor: Option<(u64, usize)>,
    scroll_to_cursor: bool,
    focus_editor: bool,
    focus_attempts: u8,
    cursor: CursorInfo,
    status: Option<(String, f64)>,
    last_title: String,
    allow_close: bool,
    /// 마지막으로 파일 변경을 확인한 때 (`ctx.input(|i| i.time)`)
    last_disk_check: f64,
}

impl NotepadApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);

        let mut settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, "settings")).unwrap_or_default();
        settings.clamp();
        let session: Session = if settings.restore_session {
            cc.storage.and_then(|s| eframe::get_value(s, "session")).unwrap_or_default()
        } else {
            Session::default()
        };

        // 처음 실행: 문서\TB Notes 를 만들고 사이드바에 추가
        let mut welcome = None;
        if !settings.roots_initialized {
            settings.roots_initialized = true;
            let dir = sidebar::default_notes_dir();
            if std::fs::create_dir_all(&dir).is_ok() {
                let file = dir.join("시작하기.md");
                if !file.exists() && write_welcome_note(&dir).is_ok() {
                    welcome = Some(file);
                }
                if !settings.roots.contains(&dir) {
                    settings.roots.push(dir);
                }
            }
        }

        let mut app = Self {
            docs: Vec::new(),
            active: 0,
            settings,
            fonts: FontLibrary::load(),
            applied: None,
            untitled_counter: 0,
            sidebar: Sidebar::default(),
            find: FindState::default(),
            dialog: None,
            show_settings: false,
            settings_page: SettingsPage::Theme,
            font_filter: String::new(),
            pending_action: None,
            pending_select: None,
            selected_image: None,
            last_cursor: None,
            scroll_to_cursor: false,
            focus_editor: true,
            focus_attempts: 0,
            cursor: CursorInfo::default(),
            status: None,
            last_title: String::new(),
            allow_close: false,
            last_disk_check: 0.0,
        };

        for sd in session.docs {
            let doc = match (&sd.path, sd.dirty, sd.text) {
                (Some(path), false, _) => match Document::open(path) {
                    Ok(d) => d,
                    Err(_) => continue,
                },
                (path, dirty, Some(text)) => {
                    let mut d = Document::new_untitled(sd.untitled_index);
                    d.path = path.clone();
                    d.text = text;
                    d.dirty = dirty;
                    d.encoding = sd.encoding;
                    d.line_ending = sd.line_ending;
                    d.restore_disk(sd.disk);
                    d
                }
                _ => continue,
            };
            let mut doc = doc;
            doc.auto_named = sd.auto_named;
            app.untitled_counter = app.untitled_counter.max(doc.untitled_index);
            app.docs.push(doc);
        }
        app.active = session.active.min(app.docs.len().saturating_sub(1));

        for f in welcome.iter().chain(files.iter()) {
            app.open_path(f);
        }
        if app.docs.is_empty() {
            app.new_untitled();
        }
        if let Some(p) = app.doc().path.clone() {
            let roots = app.settings.roots.clone();
            app.sidebar.reveal(&p, &roots);
        }
        // 글꼴은 다음 프레임부터 적용되므로, 첫 프레임 전에 미리 설정해 둡니다.
        app.apply_look(&cc.egui_ctx);
        crate::paste_hook::install(cc);
        app
    }

    fn doc(&self) -> &Document {
        &self.docs[self.active]
    }

    fn doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.active]
    }

    fn set_status(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        self.status = Some((msg.into(), ctx.input(|i| i.time)));
    }

    fn is_document_mode(&self) -> bool {
        self.settings.view_mode == ViewMode::Document && self.doc().is_markdown()
    }

    fn in_roots(&self, path: &Path) -> bool {
        self.settings.roots.iter().any(|r| path.starts_with(r))
    }

    // -----------------------------------------------------------------------
    // 문서 열고 닫기

    /// 다른 문서로 바꿀 때 이전 문서를 정리합니다:
    /// 아무것도 안 쓴 새 메모 파일은 지우고, 폴더 안의 저장된 문서는 메모리에서 내립니다
    /// (사이드바에서 언제든 다시 열 수 있음).
    fn set_active(&mut self, index: usize) {
        if index == self.active || index >= self.docs.len() {
            self.active = index.min(self.docs.len() - 1);
            return;
        }
        let prev_id = self.docs[self.active].id;
        let new_id = self.docs[index].id;
        self.active = index;
        self.save_if_needed(prev_id);
        if let Some(i) = self.docs.iter().position(|d| d.id == prev_id) {
            let d = &self.docs[i];
            let empty_new_note = d.auto_named && d.text.trim().is_empty() && !d.dirty;
            if empty_new_note {
                if let Some(p) = &d.path
                    && std::fs::metadata(p).is_ok_and(|m| m.len() == 0)
                {
                    let _ = std::fs::remove_file(p);
                    self.sidebar.invalidate();
                }
                self.docs.remove(i);
            } else if !d.dirty && d.path.as_deref().is_some_and(|p| self.in_roots(p)) {
                self.docs.remove(i);
            }
        }
        self.active = self.docs.iter().position(|d| d.id == new_id).unwrap_or(0);
        self.focus_editor = true;
        self.find.current = None;
    }

    fn push_and_activate(&mut self, doc: Document) {
        self.docs.push(doc);
        self.set_active(self.docs.len() - 1);
    }

    fn new_untitled(&mut self) {
        self.untitled_counter += 1;
        let doc = Document::new_untitled(self.untitled_counter);
        if self.docs.is_empty() {
            self.docs.push(doc);
            self.active = 0;
        } else {
            self.push_and_activate(doc);
        }
        self.focus_editor = true;
    }

    /// "새 메모": 지금 문서가 있는 폴더(또는 첫 번째 메모 폴더)에 파일을 바로 만듭니다.
    fn new_note(&mut self) {
        let dir = self
            .doc()
            .dir()
            .filter(|d| self.in_roots(d))
            .or_else(|| self.settings.roots.iter().find(|r| r.exists()).cloned());
        match dir {
            Some(dir) => self.new_note_in(&dir),
            None => self.new_untitled(),
        }
    }

    fn new_note_in(&mut self, dir: &Path) {
        let path = unique_path(dir, "새 메모", "md");
        match std::fs::write(&path, "") {
            Ok(()) => {
                if let Ok(mut doc) = Document::open(&path) {
                    doc.auto_named = true;
                    self.push_and_activate(doc);
                }
                self.sidebar.invalidate();
                let roots = self.settings.roots.clone();
                self.sidebar.reveal(&path, &roots);
                self.focus_editor = true;
            }
            Err(err) => self.dialog = Some(Dialog::Message(format!("메모를 만들 수 없습니다.\n{}\n\n{err}", dir.display()))),
        }
    }

    fn open_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("텍스트 / 마크다운", TEXT_EXTENSIONS)
            .add_filter("모든 파일", &["*"]);
        if let Some(dir) = self.doc().dir() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(paths) = dlg.pick_files() {
            for p in paths {
                self.open_path(&p);
            }
        }
    }

    fn open_path(&mut self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_file(p, &canonical))) {
            self.set_active(i);
            return;
        }
        match Document::open(path) {
            Ok(doc) => {
                if self.docs.len() == 1 && self.docs[0].is_pristine() {
                    self.docs[0] = doc;
                    self.active = 0;
                } else {
                    self.push_and_activate(doc);
                }
                self.settings.push_recent(path.to_path_buf());
                self.focus_editor = true;
            }
            Err(err) => {
                self.settings.recent_files.retain(|p| p != path);
                self.dialog = Some(Dialog::Message(format!("파일을 열 수 없습니다.\n{}\n\n{err}", path.display())));
            }
        }
    }

    fn request_close_doc(&mut self, id: u64) {
        let Some(i) = self.docs.iter().position(|d| d.id == id) else { return };
        self.save_if_needed(id);
        if self.docs[i].dirty {
            self.set_active(i);
            self.dialog = Some(Dialog::CloseDoc(id));
        } else {
            self.close_doc(id);
        }
    }

    fn close_doc(&mut self, id: u64) {
        let Some(i) = self.docs.iter().position(|d| d.id == id) else { return };
        let active_id = self.doc().id;
        self.docs.remove(i);
        if self.docs.is_empty() {
            self.untitled_counter = 0;
            self.new_untitled();
            return;
        }
        self.active = self.docs.iter().position(|d| d.id == active_id).unwrap_or(i.saturating_sub(1).min(self.docs.len() - 1));
        self.focus_editor = true;
    }

    // -----------------------------------------------------------------------
    // 저장

    /// 저장에 성공하면 `true`. 대화상자에서 취소하면 `false`.
    fn save_doc(&mut self, ctx: &egui::Context, index: usize, save_as: bool) -> bool {
        let path = match (&self.docs[index].path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let doc = &self.docs[index];
                let default_name = match &doc.path {
                    Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    None => format!("{}.md", title_from_text(&doc.text).unwrap_or_else(|| doc.title())),
                };
                let mut dlg = rfd::FileDialog::new()
                    .set_file_name(&default_name)
                    .add_filter("마크다운", &["md"])
                    .add_filter("텍스트", &["txt"])
                    .add_filter("모든 파일", &["*"]);
                if let Some(dir) = doc.dir().or_else(|| self.settings.roots.first().cloned()) {
                    dlg = dlg.set_directory(dir);
                }
                match dlg.save_file() {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        // 다른 곳에 있는 그림을 문서 폴더로 복사해 함께 보관 (다른 이름으로 저장해 폴더가 바뀐 경우 포함)
        if self.settings.keep_images_with_doc
            && is_markdown_path(&path)
            && let Some(new_dir) = path.parent()
        {
            let old_base = self.docs[index].dir();
            let folder = self.settings.image_folder.clone();
            if let Some(text) = markdown::collect_images(&self.docs[index].text, old_base.as_deref(), new_dir, &folder) {
                self.docs[index].text = text;
            }
        }
        match self.docs[index].save_to(&path) {
            Ok(fell_back) => {
                self.docs[index].highlight_cache = None;
                self.docs[index].auto_named = false;
                self.settings.push_recent(path.clone());
                self.sidebar.invalidate();
                let name = self.docs[index].title();
                if fell_back {
                    self.set_status(ctx, format!("{name}: 이 인코딩으로 표현할 수 없는 글자가 있어 UTF-8로 저장했습니다"));
                } else {
                    self.set_status(ctx, format!("{name} 저장됨"));
                }
                true
            }
            Err(err) => {
                self.dialog = Some(Dialog::Message(format!("저장하지 못했습니다.\n{}\n\n{err}", path.display())));
                false
            }
        }
    }

    fn save_all(&mut self, ctx: &egui::Context) -> bool {
        for i in 0..self.docs.len() {
            if self.docs[i].dirty && !self.save_doc(ctx, i, false) {
                return false;
            }
        }
        true
    }

    /// 파일이 있는 문서를 조용히 저장 (자동 저장). "새 메모"는 첫 줄로 파일 이름도 맞춥니다.
    fn save_if_needed(&mut self, id: u64) {
        if !self.settings.autosave {
            return;
        }
        let Some(i) = self.docs.iter().position(|d| d.id == id) else { return };
        let doc = &mut self.docs[i];
        let Some(path) = doc.path.clone() else { return };
        // 덮어쓰기 직전에 파일이 밖에서 바뀌지 않았는지 확인합니다. 바뀌었으면 저장하지 않고
        // 충돌로 표시해 두고, 사용자에게 물어봅니다 (show_conflict).
        if doc.dirty && !doc.conflict {
            doc.check_disk();
        }
        if doc.conflict {
            return;
        }
        if doc.auto_named
            && let Some(title) = title_from_text(&doc.text)
            && let Some(dir) = path.parent()
            && path.file_stem().is_some_and(|s| s.to_string_lossy() != title)
        {
            let target = unique_path(dir, &title, "md");
            if std::fs::rename(&path, &target).is_ok() {
                doc.path = Some(target);
                self.sidebar.invalidate();
            }
        }
        let doc = &mut self.docs[i];
        if doc.dirty
            && let Some(path) = doc.path.clone()
            && doc.save_to(&path).is_ok()
        {
            doc.last_edit = None;
        }
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        if !self.settings.autosave {
            return;
        }
        let due: Vec<u64> = self
            .docs
            .iter()
            .filter(|d| d.dirty && d.path.is_some())
            .filter(|d| d.last_edit.is_none_or(|t| t.elapsed().as_secs_f32() >= AUTOSAVE_DELAY))
            .map(|d| d.id)
            .collect();
        for id in due {
            self.save_if_needed(id);
        }
        if self.docs.iter().any(|d| d.dirty && d.path.is_some() && !d.conflict) {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    // -----------------------------------------------------------------------
    // 밖에서 바뀐 파일 (AI 도구, 동기화 프로그램, 다른 편집기 등)

    /// 창이 포커스를 얻을 때와 몇 초마다 열린 문서의 파일을 확인합니다.
    /// 앱에서 고친 내용이 없으면 조용히 다시 읽고, 있으면 충돌로 표시합니다.
    fn check_external_changes(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let focus_gained = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true))));
        if focus_gained || now - self.last_disk_check >= DISK_CHECK_INTERVAL {
            self.last_disk_check = now;
            for i in 0..self.docs.len() {
                if let DiskEvent::Reloaded { old_text } = self.docs[i].check_disk() {
                    self.after_reload(ctx, i, &old_text);
                    let name = self.docs[i].title();
                    self.set_status(ctx, format!("{name}: 밖에서 바뀐 내용을 불러왔습니다"));
                }
            }
        }
        // 창이 가만히 있어도 주기적으로 확인하도록
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(DISK_CHECK_INTERVAL));
    }

    /// 다시 읽은 문서의 커서/선택을 새 내용에 맞춰 옮깁니다.
    fn after_reload(&mut self, ctx: &egui::Context, index: usize, old_text: &str) {
        let doc = &self.docs[index];
        let id = editor_id(doc.id);
        if let Some(mut state) = TextEditState::load(ctx, id)
            && let Some(range) = state.cursor.char_range()
        {
            let primary = remap_char_index(old_text, &doc.text, range.primary.index.0);
            let secondary = remap_char_index(old_text, &doc.text, range.secondary.index.0);
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(secondary), CCursor::new(primary))));
            state.store(ctx, id);
        }
        if index == self.active {
            self.find.current = None;
        }
        ctx.request_repaint();
    }

    /// 충돌한 문서가 있으면 (다른 대화상자가 없을 때) 어떻게 할지 묻습니다.
    fn show_conflict(&mut self) {
        if self.dialog.is_none()
            && let Some(d) = self.docs.iter().find(|d| d.conflict)
        {
            self.dialog = Some(Dialog::ExternalChange(d.id));
        }
    }

    fn export_html(&mut self, ctx: &egui::Context) {
        let doc = self.doc();
        let stem = doc
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| doc.title());
        let mut dlg = rfd::FileDialog::new().set_file_name(format!("{stem}.html")).add_filter("HTML", &["html"]);
        if let Some(dir) = doc.dir() {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.save_file() else { return };
        let html = markdown::to_html(&doc.text, &stem, &self.settings.palette());
        match std::fs::write(&path, html) {
            Ok(()) => self.set_status(ctx, format!("{} 로 내보냈습니다", path.display())),
            Err(err) => self.dialog = Some(Dialog::Message(format!("내보내지 못했습니다.\n{err}"))),
        }
    }

    // -----------------------------------------------------------------------
    // 사이드바 동작

    fn handle_sidebar(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Open(p) => self.open_path(&p),
                Action::NewNote(dir) => self.new_note_in(&dir),
                Action::NewQuickNote => self.new_note(),
                Action::NewFolder(parent) => self.dialog = Some(Dialog::NewFolder { parent, name: "새 폴더".into() }),
                Action::AddRoot => {
                    if let Some(dir) = rfd::FileDialog::new().set_title("메모 폴더 고르기").pick_folder()
                        && !self.settings.roots.contains(&dir)
                    {
                        self.settings.roots.push(dir);
                        self.sidebar.invalidate();
                    }
                }
                Action::RemoveRoot(dir) => self.settings.roots.retain(|r| *r != dir),
                Action::Rename { from, to_name } => self.rename_path(ctx, &from, &to_name),
                Action::Delete(p) => self.dialog = Some(Dialog::ConfirmDelete(p)),
                Action::Reveal(p) => {
                    let _ = reveal_in_file_manager(&p);
                }
                Action::ActivateDoc(id) => {
                    if let Some(i) = self.docs.iter().position(|d| d.id == id) {
                        self.set_active(i);
                    }
                }
                Action::CloseDoc(id) => self.request_close_doc(id),
            }
        }
    }

    fn rename_path(&mut self, ctx: &egui::Context, from: &Path, to_name: &str) {
        let clean = sanitize_file_name(to_name);
        if clean.is_empty() {
            return;
        }
        let Some(dir) = from.parent() else { return };
        let target = if from.is_dir() || Path::new(&clean).extension().is_some() {
            dir.join(&clean)
        } else {
            let ext = from.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "md".into());
            dir.join(format!("{clean}.{ext}"))
        };
        if target.exists() {
            self.set_status(ctx, "같은 이름이 이미 있습니다");
            return;
        }
        // 열려 있는 문서는 먼저 저장해 두고 경로를 함께 바꿉니다.
        let ids: Vec<u64> = self.docs.iter().filter(|d| d.path.as_deref().is_some_and(|p| p.starts_with(from))).map(|d| d.id).collect();
        for id in &ids {
            self.save_if_needed(*id);
        }
        match std::fs::rename(from, &target) {
            Ok(()) => {
                for d in &mut self.docs {
                    if let Some(p) = &d.path
                        && let Ok(rest) = p.strip_prefix(from)
                    {
                        d.path = Some(if rest.as_os_str().is_empty() { target.clone() } else { target.join(rest) });
                        d.auto_named = false;
                    }
                }
                self.sidebar.invalidate();
            }
            Err(err) => self.dialog = Some(Dialog::Message(format!("이름을 바꾸지 못했습니다.\n{err}"))),
        }
    }

    fn delete_path(&mut self, ctx: &egui::Context, path: &Path) {
        match sidebar::move_to_trash(path) {
            Ok(()) => {
                let ids: Vec<u64> = self.docs.iter().filter(|d| d.path.as_deref().is_some_and(|p| p.starts_with(path))).map(|d| d.id).collect();
                for id in ids {
                    self.close_doc(id);
                }
                self.sidebar.invalidate();
                self.set_status(ctx, "휴지통으로 옮겼습니다");
            }
            Err(err) => self.dialog = Some(Dialog::Message(format!("지우지 못했습니다.\n{err}"))),
        }
    }

    // -----------------------------------------------------------------------
    // 이미지

    /// 문서 폴더 밖의 그림이면 (설정에 따라) 문서 옆 그림 폴더로 복사해 그 복사본을 씁니다.
    fn adopt_image(&self, image: &Path) -> PathBuf {
        let Some(dir) = self.doc().dir() else { return image.to_path_buf() };
        if !self.settings.keep_images_with_doc || image.starts_with(&dir) {
            return image.to_path_buf();
        }
        let target_dir = dir.join(&self.settings.image_folder);
        let name = image.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
        let ext = image.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "png".into());
        if std::fs::create_dir_all(&target_dir).is_err() {
            return image.to_path_buf();
        }
        let target = unique_path(&target_dir, &name, &ext);
        match std::fs::copy(image, &target) {
            Ok(_) => target,
            Err(_) => image.to_path_buf(),
        }
    }

    /// 문서에서 이미지 파일을 가리킬 경로 (가능하면 문서 기준 상대 경로)
    fn image_link(&self, image: &Path) -> String {
        let image = &self.adopt_image(image);
        let name = image.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let target = match self.doc().dir() {
            Some(dir) => image.strip_prefix(&dir).map(Path::to_path_buf).unwrap_or_else(|_| image.to_path_buf()),
            None => image.to_path_buf(),
        };
        let mut s = target.to_string_lossy().replace('\\', "/");
        if s.contains([' ', '(', ')', '<', '>']) {
            s = format!("<{s}>");
        }
        format!("![{name}]({s})")
    }

    /// 그림은 한 줄을 다 차지하도록 앞뒤에 줄바꿈을 붙여 넣습니다.
    fn insert_image_links(&mut self, links: Vec<String>) {
        if links.is_empty() || !self.doc().is_markdown() {
            return;
        }
        self.pending_action = Some(MdAction::Insert(format!("\n{}\n", links.join("\n"))));
        self.focus_editor = true;
    }

    fn insert_image_dialog(&mut self) {
        if !self.doc().is_markdown() {
            return;
        }
        let mut dlg = rfd::FileDialog::new().add_filter("이미지", IMAGE_EXTENSIONS);
        if let Some(dir) = self.doc().dir() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(paths) = dlg.pick_files() {
            let links = paths.iter().map(|p| self.image_link(p)).collect();
            self.insert_image_links(links);
        }
    }

    fn paste_clipboard_image(&mut self, ctx: &egui::Context) {
        if !self.doc().is_markdown() {
            self.set_status(ctx, "텍스트 파일(.txt)에는 그림을 넣을 수 없습니다");
            return;
        }
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };
        let image = match clipboard.get_image() {
            Ok(img) => img,
            Err(_) => {
                // 탐색기에서 복사한 이미지 파일이면 그 파일을 가리키는 링크를 넣습니다.
                let files = clipboard.get().file_list().unwrap_or_default();
                let links: Vec<String> =
                    files.iter().filter(|p| has_extension(p, IMAGE_EXTENSIONS)).map(|p| self.image_link(p)).collect();
                if links.is_empty() {
                    self.set_status(ctx, "클립보드에 이미지가 없습니다");
                } else {
                    self.set_status(ctx, format!("이미지 {}개를 넣었습니다", links.len()));
                    self.insert_image_links(links);
                }
                return;
            }
        };
        let dir = match self.doc().dir() {
            Some(d) => d.join(&self.settings.image_folder),
            None => app_data_dir().join("images"),
        };
        let file = dir.join(format!("image_{}.png", chrono::Local::now().format("%Y%m%d_%H%M%S")));
        let result = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| {
            let buf = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())
                .ok_or_else(|| "이미지 형식이 올바르지 않습니다".to_owned())?;
            buf.save_with_format(&file, image::ImageFormat::Png).map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => {
                let link = self.image_link(&file);
                self.insert_image_links(vec![link]);
                self.set_status(ctx, "이미지를 붙여넣었습니다");
            }
            Err(err) => self.dialog = Some(Dialog::Message(format!("이미지를 저장하지 못했습니다.\n{err}"))),
        }
    }

    // -----------------------------------------------------------------------
    // 모양

    fn apply_look(&mut self, ctx: &egui::Context) {
        let look = Look {
            palette: self.settings.palette(),
            editor_font: self.settings.editor_font.clone(),
            ui_font: self.settings.ui_font.clone(),
            doc_font: self.settings.doc_font.clone(),
            ui_scale: self.settings.ui_scale,
            always_on_top: self.settings.always_on_top,
        };
        if self.applied.as_ref() == Some(&look) {
            return;
        }
        let prev = self.applied.replace(look.clone());
        let fonts_changed = prev
            .as_ref()
            .is_none_or(|p| p.editor_font != look.editor_font || p.ui_font != look.ui_font || p.doc_font != look.doc_font);
        if fonts_changed {
            ctx.set_fonts(self.fonts.definitions(look.editor_font.as_deref(), look.ui_font.as_deref(), look.doc_font.as_deref()));
        }
        look.palette.apply(ctx);
        if prev.as_ref().is_none_or(|p| p.ui_scale != look.ui_scale) {
            ctx.set_zoom_factor(look.ui_scale);
        }
        if prev.as_ref().is_none_or(|p| p.always_on_top != look.always_on_top) {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if look.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }));
        }
        for d in &mut self.docs {
            d.highlight_cache = None;
            d.live_cache = None;
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let doc = self.doc();
        let unsaved = doc.dirty && (doc.path.is_none() || !self.settings.autosave);
        let title = format!("{}{} - {APP_NAME}", if unsaved { "● " } else { "" }, doc_display_title(doc));
        if title != self.last_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn zoom_editor(&mut self, factor: f32) {
        self.settings.editor_font_size = (self.settings.editor_font_size * factor).clamp(6.0, 96.0);
    }

    fn toggle_source_view(&mut self) {
        self.settings.view_mode = match self.settings.view_mode {
            ViewMode::Document => ViewMode::Source,
            ViewMode::Source => ViewMode::Document,
        };
    }

    // -----------------------------------------------------------------------
    // 입력

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);
        let cmd_alt = Modifiers::COMMAND.plus(Modifiers::ALT);
        let sc = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));

        // Shift/Alt가 붙은 단축키를 먼저 확인해야 Ctrl+S 등에 가로채이지 않습니다.
        if sc(cmd_shift, Key::S) {
            self.save_doc(ctx, self.active, true);
        }
        if sc(cmd_shift, Key::I) {
            self.paste_clipboard_image(ctx);
        }
        for (k, lvl) in [(Key::Num1, 1), (Key::Num2, 2), (Key::Num3, 3)] {
            if sc(cmd_alt, k) {
                self.pending_action = Some(MdAction::Heading(lvl));
            }
        }
        if sc(cmd, Key::N) {
            self.new_note();
        }
        if sc(cmd, Key::O) {
            self.open_dialog();
        }
        if sc(cmd, Key::S) {
            self.save_doc(ctx, self.active, false);
        }
        if sc(cmd, Key::W) {
            self.request_close_doc(self.doc().id);
        }
        if sc(cmd, Key::P) {
            self.settings.sidebar_visible = true;
            ctx.memory_mut(|m| m.request_focus(Id::new("sidebar_search")));
        }
        if sc(cmd, Key::Backslash) {
            self.settings.sidebar_visible = !self.settings.sidebar_visible;
        }
        if sc(cmd, Key::F) {
            self.open_find(ctx, false);
        }
        if sc(cmd, Key::H) {
            self.open_find(ctx, true);
        }
        if sc(Modifiers::NONE, Key::F3) {
            self.find_step(ctx, true);
        }
        if sc(Modifiers::SHIFT, Key::F3) {
            self.find_step(ctx, false);
        }
        if sc(cmd, Key::B) {
            self.pending_action = Some(MdAction::Bold);
        }
        if sc(cmd, Key::I) {
            self.pending_action = Some(MdAction::Italic);
        }
        if sc(cmd, Key::K) {
            self.pending_action = Some(MdAction::Link);
        }
        if sc(cmd, Key::E) {
            self.toggle_source_view();
        }
        if sc(cmd, Key::Equals) || sc(cmd, Key::Plus) {
            self.zoom_editor(1.1);
        }
        if sc(cmd, Key::Minus) {
            self.zoom_editor(1.0 / 1.1);
        }
        if sc(cmd, Key::Num0) {
            self.settings.editor_font_size = Settings::default().editor_font_size;
        }
        if sc(cmd, Key::Comma) {
            self.show_settings = !self.show_settings;
        }
        if sc(Modifiers::ALT, Key::Z) {
            self.settings.word_wrap = !self.settings.word_wrap;
        }
        if sc(Modifiers::NONE, Key::F5) {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
            self.pending_action = Some(MdAction::Insert(now));
        }
        if sc(Modifiers::NONE, Key::F2)
            && let Some(p) = self.doc().path.clone()
        {
            self.settings.sidebar_visible = true;
            self.sidebar.start_rename(&p);
        }
        if sc(Modifiers::NONE, Key::F11) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
        }
        if self.find.open && self.dialog.is_none() && sc(Modifiers::NONE, Key::Escape) {
            self.close_find();
        }

        // Ctrl + 마우스 휠로 글자 크기 조절
        let zoom = ctx.input(|i| i.zoom_delta());
        if zoom != 1.0 {
            self.zoom_editor(zoom);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        let mut images = Vec::new();
        for path in dropped {
            if path.is_dir() {
                // 폴더를 끌어다 놓으면 메모 폴더로 추가
                if !self.settings.roots.contains(&path) {
                    self.settings.roots.push(path);
                    self.sidebar.invalidate();
                }
                continue;
            }
            if has_extension(&path, IMAGE_EXTENSIONS) {
                if !self.doc().is_markdown() {
                    self.set_status(ctx, "텍스트 파일(.txt)에는 그림을 넣을 수 없습니다");
                    continue;
                }
                images.push(self.image_link(&path));
            } else {
                self.open_path(&path);
            }
        }
        self.insert_image_links(images);
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        // 파일이 있는 문서는 자동 저장, 새 문서는 세션 복원으로 다음 실행 때 돌아옵니다.
        let ids: Vec<u64> = self.docs.iter().map(|d| d.id).collect();
        for id in ids {
            self.save_if_needed(id);
        }
        if self.settings.restore_session || !self.docs.iter().any(|d| d.dirty) {
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        self.dialog = Some(Dialog::Exit);
    }

    // -----------------------------------------------------------------------
    // 찾기

    fn open_find(&mut self, ctx: &egui::Context, replace: bool) {
        self.find.open = true;
        self.find.replace = replace;
        self.find.focus_query = true;
        self.prefill_find_query(ctx);
    }

    fn prefill_find_query(&mut self, ctx: &egui::Context) {
        let id = editor_id(self.doc().id);
        if let Some(range) = TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()) {
            let r = range.as_sorted_char_range();
            let (s, e) = (r.start.0, r.end.0);
            if s != e {
                let text = &self.doc().text;
                let sel = &text[markdown::char_to_byte(text, s)..markdown::char_to_byte(text, e)];
                if !sel.contains('\n') {
                    self.find.query = sel.to_owned();
                }
            }
        }
    }

    fn refresh_matches(&mut self) {
        let doc = &self.docs[self.active];
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            doc.id.hash(&mut h);
            doc.text.hash(&mut h);
            self.find.query.hash(&mut h);
            self.find.match_case.hash(&mut h);
            h.finish()
        };
        if key == self.find.matches_key {
            return;
        }
        self.find.matches_key = key;
        let byte_ranges = markdown::find_all(&doc.text, &self.find.query, self.find.match_case);
        // 바이트 → 글자 위치 (한 번만 훑기)
        let mut out = Vec::with_capacity(byte_ranges.len());
        let mut chars = 0;
        let mut last_byte = 0;
        for r in byte_ranges {
            chars += doc.text[last_byte..r.start].chars().count();
            let len = doc.text[r.clone()].chars().count();
            out.push(chars..chars + len);
            chars += len;
            last_byte = r.end;
        }
        self.find.matches = out;
    }

    fn current_selection(&self, ctx: &egui::Context) -> Range<usize> {
        let id = editor_id(self.doc().id);
        TextEditState::load(ctx, id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let r = r.as_sorted_char_range();
                r.start.0..r.end.0
            })
            .unwrap_or(0..0)
    }

    fn find_step(&mut self, ctx: &egui::Context, forward: bool) {
        if self.find.query.is_empty() {
            self.find.open = true;
            self.find.focus_query = true;
            return;
        }
        self.refresh_matches();
        if self.find.matches.is_empty() {
            self.set_status(ctx, format!("'{}'을(를) 찾을 수 없습니다", self.find.query));
            return;
        }
        let base = self.find.current.clone().unwrap_or_else(|| self.current_selection(ctx));
        let m = &self.find.matches;
        let next = if forward {
            m.iter().find(|r| r.start >= base.end && **r != base).or(m.first())
        } else {
            m.iter().rev().find(|r| r.end <= base.start && **r != base).or(m.last())
        };
        self.find.current = next.cloned();
        // 커서를 옮겨 화면을 그 위치로 스크롤
        self.pending_select = next.cloned();
    }

    fn close_find(&mut self) {
        self.find.open = false;
        // 찾은 부분을 선택한 채로 편집기로 돌아갑니다.
        if let Some(r) = self.find.current.take() {
            self.pending_select = Some(r);
        }
        self.focus_editor = true;
    }

    fn replace_one(&mut self, ctx: &egui::Context) {
        self.refresh_matches();
        let current = self.find.current.clone().filter(|r| self.find.matches.contains(r));
        let Some(r) = current else {
            self.find_step(ctx, true);
            return;
        };
        let replacement = self.find.replacement.clone();
        let doc = self.doc_mut();
        let range = markdown::char_to_byte(&doc.text, r.start)..markdown::char_to_byte(&doc.text, r.end);
        doc.text.replace_range(range, &replacement);
        doc.mark_changed();
        let after = r.start + replacement.chars().count();
        self.find.current = Some(after..after);
        self.refresh_matches();
        if self.find.matches.is_empty() {
            self.find.current = None;
            self.pending_select = Some(after..after);
        } else {
            self.find_step(ctx, true);
        }
    }

    fn replace_all(&mut self, ctx: &egui::Context) {
        let ranges = markdown::find_all(&self.doc().text, &self.find.query, self.find.match_case);
        if ranges.is_empty() {
            self.set_status(ctx, "바꿀 내용이 없습니다");
            return;
        }
        let replacement = self.find.replacement.clone();
        let doc = self.doc_mut();
        let mut out = String::with_capacity(doc.text.len());
        let mut last = 0;
        for r in &ranges {
            out.push_str(&doc.text[last..r.start]);
            out.push_str(&replacement);
            last = r.end;
        }
        out.push_str(&doc.text[last..]);
        doc.text = out;
        doc.mark_changed();
        self.set_status(ctx, format!("{}개를 바꿨습니다", ranges.len()));
    }

    // -----------------------------------------------------------------------
    // 화면 구성

    /// ⋯ 버튼 메뉴
    fn main_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.set_min_width(250.0);
        if item(ui, "새 메모", "Ctrl+N") {
            self.new_note();
        }
        if item(ui, "파일 열기…", "Ctrl+O") {
            self.open_dialog();
        }
        ui.menu_button("최근 파일", |ui| {
            if self.settings.recent_files.is_empty() {
                ui.label(RichText::new("최근 파일이 없습니다").weak());
            }
            for p in self.settings.recent_files.clone() {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if ui.button(name).on_hover_text(p.display().to_string()).clicked() {
                    self.open_path(&p);
                    ui.close();
                }
            }
            if !self.settings.recent_files.is_empty() {
                ui.separator();
                if ui.button("목록 지우기").clicked() {
                    self.settings.recent_files.clear();
                }
            }
        });
        ui.separator();
        if item(ui, "저장", "Ctrl+S") {
            self.save_doc(&ctx, self.active, false);
        }
        if item(ui, "다른 이름으로 저장…", "Ctrl+Shift+S") {
            self.save_doc(&ctx, self.active, true);
        }
        if item(ui, "HTML로 내보내기…", "") {
            self.export_html(&ctx);
        }
        ui.separator();
        ui.menu_button("편집", |ui| {
            if item(ui, "찾기", "Ctrl+F") {
                self.open_find(&ctx, false);
            }
            if item(ui, "바꾸기", "Ctrl+H") {
                self.open_find(&ctx, true);
            }
            if item(ui, "다음 찾기", "F3") {
                self.find_step(&ctx, true);
            }
            ui.separator();
            if item(ui, "날짜/시간 넣기", "F5") {
                let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                self.pending_action = Some(MdAction::Insert(now));
            }
            if item(ui, "그림 파일 넣기…", "") {
                self.insert_image_dialog();
            }
            if item(ui, "클립보드 그림 붙여넣기", "Ctrl+V") {
                self.paste_clipboard_image(&ctx);
            }
        });
        let is_md = self.doc().is_markdown();
        ui.add_enabled_ui(is_md, |ui| ui.menu_button("서식", |ui| {
            for (label, key, action) in [
                ("굵게", "Ctrl+B", MdAction::Bold),
                ("기울임", "Ctrl+I", MdAction::Italic),
                ("취소선", "", MdAction::Strike),
                ("인라인 코드", "", MdAction::Code),
                ("링크", "Ctrl+K", MdAction::Link),
            ] {
                if item(ui, label, key) {
                    self.pending_action = Some(action);
                }
            }
            ui.separator();
            for lvl in 1..=3 {
                if item(ui, &format!("제목 {lvl}"), &format!("Ctrl+Alt+{lvl}")) {
                    self.pending_action = Some(MdAction::Heading(lvl));
                }
            }
            ui.separator();
            for (label, action) in [
                ("글머리 목록", MdAction::Bullet),
                ("번호 목록", MdAction::Numbered),
                ("할 일 목록", MdAction::Task),
                ("인용", MdAction::Quote),
                ("코드 블록", MdAction::CodeBlock),
                ("표", MdAction::Table),
                ("구분선", MdAction::Rule),
            ] {
                if item(ui, label, "") {
                    self.pending_action = Some(action);
                }
            }
        }));
        ui.menu_button("보기", |ui| {
            let mut source = self.settings.view_mode == ViewMode::Source;
            if ui.checkbox(&mut source, "마크다운 원본 보기  (Ctrl+E)").changed() {
                self.toggle_source_view();
            }
            ui.checkbox(&mut self.settings.sidebar_visible, "사이드바  (Ctrl+\\)");
            ui.checkbox(&mut self.settings.show_line_numbers, "줄 번호");
            ui.checkbox(&mut self.settings.show_toolbar, "서식 도구");
            ui.checkbox(&mut self.settings.show_status_bar, "상태 표시줄");
            ui.checkbox(&mut self.settings.word_wrap, "원본 보기에서 자동 줄 바꿈  (Alt+Z)");
            ui.separator();
            if item(ui, "글자 크게", "Ctrl+=") {
                self.zoom_editor(1.1);
            }
            if item(ui, "글자 작게", "Ctrl+-") {
                self.zoom_editor(1.0 / 1.1);
            }
            if item(ui, "기본 크기", "Ctrl+0") {
                self.settings.editor_font_size = Settings::default().editor_font_size;
            }
            ui.separator();
            ui.checkbox(&mut self.settings.always_on_top, "항상 위에 표시");
            if item(ui, "전체 화면", "F11") {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
            }
        });
        ui.menu_button("파일 형식", |ui| {
            let doc = &mut self.docs[self.active];
            ui.label(RichText::new("인코딩").small().weak());
            for enc in TextEncoding::ALL {
                if ui.radio(doc.encoding == enc, enc.label()).clicked() && doc.encoding != enc {
                    doc.encoding = enc;
                    doc.mark_changed();
                }
            }
            ui.separator();
            ui.label(RichText::new("줄 끝").small().weak());
            for le in [LineEnding::CrLf, LineEnding::Lf] {
                if ui.radio(doc.line_ending == le, le.label()).clicked() && doc.line_ending != le {
                    doc.line_ending = le;
                    doc.mark_changed();
                }
            }
        });
        ui.separator();
        if item(ui, "설정", "Ctrl+,") {
            self.open_settings(SettingsPage::Theme);
        }
        if item(ui, "단축키", "") {
            self.open_settings(SettingsPage::Keys);
        }
        if item(ui, "정보", "") {
            self.open_settings(SettingsPage::About);
        }
        ui.separator();
        if item(ui, "끝내기", "Alt+F4") {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn open_settings(&mut self, page: SettingsPage) {
        self.show_settings = true;
        self.settings_page = page;
    }

    fn sidebar_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let k = ui_kit::colors(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.label(RichText::new(APP_NAME).size(14.5).strong());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui_kit::icon_button(ui, Icon::Sidebar, "사이드바 닫기 (Ctrl+\\)", false, k.panel).clicked() {
                    self.settings.sidebar_visible = false;
                }
            });
        });
        ui.add_space(8.0);
        let active = self.doc().path.clone();
        let loose: Vec<LooseDoc> = self
            .docs
            .iter()
            .enumerate()
            .filter(|(_, d)| d.path.as_deref().is_none_or(|p| !self.in_roots(p)))
            .map(|(i, d)| LooseDoc {
                id: d.id,
                title: doc_display_title(d),
                dirty: d.dirty && (d.path.is_none() || !self.settings.autosave),
                active: i == self.active,
            })
            .collect();
        let roots = self.settings.roots.clone();
        let folder = self.settings.image_folder.clone();
        let actions = self.sidebar.show(ui, &roots, active.as_deref(), &loose, &folder);
        self.handle_sidebar(&ctx, actions);
    }

    /// 본문 위 한 줄: (사이드바 열기) 폴더 › 제목 · 찾기 · 원본 보기 · 테마 · 설정 · ⋯
    fn top_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let k = ui_kit::colors(ui);
        let base = k.editor;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if !self.settings.sidebar_visible
                && ui_kit::icon_button(ui, Icon::Sidebar, "사이드바 열기 (Ctrl+\\)", false, base).clicked()
            {
                self.settings.sidebar_visible = true;
            }
            ui.add_space(6.0);
            // 폴더 경로 › 제목
            let doc = self.doc();
            let mut crumbs = Vec::new();
            if let Some(p) = &doc.path
                && let Some(root) = self.settings.roots.iter().find(|r| p.starts_with(r))
            {
                crumbs.push(root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                if let Ok(rel) = p.strip_prefix(root) {
                    let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                    crumbs.extend(comps.iter().take(comps.len().saturating_sub(1)).cloned());
                }
            }
            for c in &crumbs {
                ui.label(RichText::new(c).size(13.0).color(k.muted));
                ui.label(RichText::new("/").size(13.0).color(mix(k.muted, k.editor, 0.4)));
            }
            ui.label(RichText::new(doc_display_title(doc)).size(13.5).color(k.text));

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let more = ui_kit::icon_button(ui, Icon::More, "메뉴", false, base);
                egui::Popup::menu(&more)
                    .info(egui::UiStackInfo::new(egui::UiKind::Menu))
                    .gap(4.0)
                    .show(|ui| self.main_menu(ui));
                if ui_kit::icon_button(ui, Icon::Settings, "설정 (Ctrl+,)", self.show_settings, base).clicked() {
                    self.show_settings = !self.show_settings;
                }
                let dark = self.settings.palette().dark;
                let theme_icon = if dark { Icon::Sun } else { Icon::Moon };
                if ui_kit::icon_button(ui, theme_icon, "밝게/어둡게", false, base).clicked() {
                    self.settings.theme = if dark { ThemePreset::Light } else { ThemePreset::Dark };
                }
                if self.doc().is_markdown() {
                    let source = self.settings.view_mode == ViewMode::Source;
                    if ui_kit::icon_button(ui, Icon::Source, "마크다운 원본 보기 (Ctrl+E)", source, base).clicked() {
                        self.toggle_source_view();
                    }
                }
                if ui_kit::icon_button(ui, Icon::Search, "찾기 (Ctrl+F)", self.find.open, base).clicked() {
                    if self.find.open {
                        self.close_find();
                    } else {
                        self.open_find(&ctx, false);
                    }
                }
            });
        });
    }

    /// 본문 열에 맞춘 얇은 서식 도구 줄
    fn toolbar(&mut self, ui: &mut Ui) {
        let k = ui_kit::colors(ui);
        let base = k.editor;
        let wrap = self.settings.word_wrap || self.is_document_mode();
        let side = self.column(ui.available_width(), wrap).0;
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            ui.add_space(side + 8.0);
            for (text, tip, a) in [
                (RichText::new("B").strong(), "굵게 (Ctrl+B)", MdAction::Bold),
                (RichText::new("I").italics(), "기울임 (Ctrl+I)", MdAction::Italic),
                (RichText::new("S").strikethrough(), "취소선", MdAction::Strike),
                (RichText::new("</>").monospace(), "인라인 코드", MdAction::Code),
            ] {
                if ui_kit::text_button(ui, text, tip, false, base).clicked() {
                    action = Some(a);
                }
            }
            ui.add_space(10.0);
            for lvl in 1..=3 {
                let tip = format!("제목 {lvl} (Ctrl+Alt+{lvl})");
                if ui_kit::text_button(ui, RichText::new(format!("H{lvl}")).size(13.0), &tip, false, base).clicked() {
                    action = Some(MdAction::Heading(lvl));
                }
            }
            ui.add_space(10.0);
            for (icon, tip, a) in [
                (Icon::Bullets, "글머리 목록", MdAction::Bullet),
                (Icon::Numbers, "번호 목록", MdAction::Numbered),
                (Icon::Task, "할 일 목록", MdAction::Task),
                (Icon::Quote, "인용", MdAction::Quote),
            ] {
                if ui_kit::icon_button(ui, icon, tip, false, base).clicked() {
                    action = Some(a);
                }
            }
            ui.add_space(10.0);
            if ui_kit::text_button(ui, RichText::new("{ }").monospace(), "코드 블록", false, base).clicked() {
                action = Some(MdAction::CodeBlock);
            }
            for (icon, tip, a) in [
                (Icon::Table, "표 넣기", MdAction::Table),
                (Icon::Rule, "구분선", MdAction::Rule),
                (Icon::Link, "링크 (Ctrl+K)", MdAction::Link),
            ] {
                if ui_kit::icon_button(ui, icon, tip, false, base).clicked() {
                    action = Some(a);
                }
            }
            if ui_kit::icon_button(ui, Icon::Image, "그림 넣기\n복사한 그림은 Ctrl+V, 파일은 창에 끌어다 놓아도 됩니다", false, base)
                .clicked()
            {
                self.insert_image_dialog();
            }
        });
        if action.is_some() {
            self.pending_action = action;
        }
    }

    /// 편집기 오른쪽 위에 떠 있는 찾기/바꾸기 카드
    fn find_overlay(&mut self, ctx: &egui::Context, area: egui::Rect) {
        self.refresh_matches();
        let current = self.find.current.as_ref().and_then(|c| self.find.matches.iter().position(|m| m == c));
        let width = 420.0_f32.min(area.width() - 24.0).max(280.0);
        let pos = egui::pos2(area.right() - width - 18.0, area.top() + 10.0);
        let mut go: Option<bool> = None;
        let mut do_replace = false;
        let mut do_replace_all = false;

        egui::Area::new(Id::new("find_overlay")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            Frame::popup(ui.style()).inner_margin(Margin::same(8)).corner_radius(10).show(ui, |ui| {
                ui.set_width(width - 16.0);
                let base = ui.visuals().window_fill;
                let field_w = (width - 16.0 - 5.0 * 32.0 - 56.0).max(110.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.find.query)
                            .desired_width(field_w)
                            .hint_text("찾기")
                            .margin(Margin::symmetric(8, 5)),
                    );
                    if self.find.focus_query {
                        resp.request_focus();
                        self.find.focus_query = false;
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        go = Some(!ui.input(|i| i.modifiers.shift));
                        resp.request_focus();
                    }
                    let count = self.find.matches.len();
                    let info = match (count, current) {
                        (0, _) if !self.find.query.is_empty() => "없음".to_owned(),
                        (0, _) => String::new(),
                        (n, Some(i)) => format!("{}/{n}", i + 1),
                        (n, None) => format!("{n}개"),
                    };
                    ui.add_sized([52.0, 26.0], egui::Label::new(RichText::new(info).small().weak()));
                    if ui_kit::text_button(ui, RichText::new("Aa").size(13.0), "대소문자 구분", self.find.match_case, base).clicked() {
                        self.find.match_case = !self.find.match_case;
                    }
                    if ui_kit::icon_button(ui, Icon::ChevronUp, "이전 (Shift+Enter, Shift+F3)", false, base).clicked() {
                        go = Some(false);
                    }
                    if ui_kit::icon_button(ui, Icon::ChevronDown, "다음 (Enter, F3)", false, base).clicked() {
                        go = Some(true);
                    }
                    if ui_kit::icon_button(ui, Icon::Replace, "바꾸기 (Ctrl+H)", self.find.replace, base).clicked() {
                        self.find.replace = !self.find.replace;
                    }
                    if ui_kit::icon_button(ui, Icon::Close, "닫기 (Esc)", false, base).clicked() {
                        self.close_find();
                    }
                });
                if self.find.replace {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let resp = ui.add(
                            TextEdit::singleline(&mut self.find.replacement)
                                .desired_width(field_w)
                                .hint_text("바꿀 내용")
                                .margin(Margin::symmetric(8, 5)),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            do_replace = true;
                            resp.request_focus();
                        }
                        ui.add_space(54.0);
                        if ui.button("바꾸기").clicked() {
                            do_replace = true;
                        }
                        if ui.button("모두 바꾸기").clicked() {
                            do_replace_all = true;
                        }
                    });
                }
            });
        });
        if let Some(forward) = go {
            self.find_step(ctx, forward);
        }
        if do_replace {
            self.replace_one(ctx);
        }
        if do_replace_all {
            self.replace_all(ctx);
        }
    }

    /// 아래쪽 흐린 글씨 한 줄
    fn status_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let flat = |text: String| egui::Button::new(RichText::new(text).small().weak()).frame_when_inactive(false);
        let autosave = self.settings.autosave;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let cur = self.cursor;
            ui.label(RichText::new(format!("줄 {}, 열 {}", cur.line.max(1), cur.column.max(1))).small().weak());
            if cur.selected > 0 {
                ui.label(RichText::new(format!("{}자 선택", fmt_num(cur.selected))).small().weak());
            }
            let doc = self.doc();
            let chars = doc.text.chars().count();
            ui.label(RichText::new(format!("{}자 · {}단어", fmt_num(chars), fmt_num(markdown::word_count(&doc.text)))).small().weak());
            let save_state = match (doc.path.is_some(), doc.dirty) {
                _ if doc.conflict => "자동 저장 멈춤 (파일이 밖에서 바뀜)",
                (true, false) => "저장됨",
                (true, true) if autosave => "저장 중…",
                (true, true) => "저장 안 됨",
                (false, _) => "파일로 저장 안 됨",
            };
            ui.label(RichText::new(save_state).small().weak());
            if !doc.is_markdown() {
                ui.label(RichText::new("일반 텍스트").small().weak());
            }
            if let Some((msg, t)) = &self.status
                && now - t < 5.0
            {
                ui.label(RichText::new(msg).small().color(c(self.settings.palette().accent)));
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let size = self.settings.editor_font_size;
                let pct = (size / Settings::default().editor_font_size * 100.0).round();
                if ui.add(flat(format!("{pct}%"))).on_hover_text("글자 크기 (Ctrl+휠) · 누르면 기본 크기").clicked() {
                    self.settings.editor_font_size = Settings::default().editor_font_size;
                }
                let doc = &mut self.docs[self.active];
                let le = match doc.line_ending {
                    LineEnding::CrLf => "CRLF",
                    LineEnding::Lf => "LF",
                };
                egui::containers::menu::MenuButton::from_button(flat(le.to_owned())).ui(ui, |ui| {
                    for le in [LineEnding::CrLf, LineEnding::Lf] {
                        if ui.radio(doc.line_ending == le, le.label()).clicked() && doc.line_ending != le {
                            doc.line_ending = le;
                            doc.mark_changed();
                            ui.close();
                        }
                    }
                });
                egui::containers::menu::MenuButton::from_button(flat(doc.encoding.label().to_owned())).ui(ui, |ui| {
                    ui.label(RichText::new("저장할 때 쓸 인코딩").small().weak());
                    for enc in TextEncoding::ALL {
                        if ui.radio(doc.encoding == enc, enc.label()).clicked() && doc.encoding != enc {
                            doc.encoding = enc;
                            doc.mark_changed();
                            ui.close();
                        }
                    }
                });
            });
        });
    }

    /// 본문 열 위치: (왼쪽 여백, 열 너비)
    fn column(&self, avail: f32, wrap: bool) -> (f32, f32) {
        let max = self.settings.max_text_width;
        if !wrap || max <= 0.0 || avail <= max {
            return (0.0, avail);
        }
        (((avail - max) / 2.0).floor(), max)
    }

    fn editor(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let palette = self.settings.palette();
        let s = self.settings.clone();
        let doc_mode = self.is_document_mode();
        let wrap = s.word_wrap || doc_mode;
        let show_numbers = s.show_line_numbers;
        let highlight_line = s.highlight_current_line && !doc_mode;
        let source_font = FontId::new(s.editor_font_size, editor_family());
        let style = HighlightStyle::new(&palette, source_font.clone(), s.line_spacing, s.markdown_highlight && self.doc().is_markdown());
        let id = editor_id(self.doc().id);
        let (side, col_w) = self.column(ui.available_width(), wrap);

        // 본문 양옆 빈 곳을 눌러도 편집기로 (TextEdit보다 먼저 등록해야 TextEdit 클릭을 가로채지 않음)
        if ui.interact(ui.max_rect(), id.with("background"), egui::Sense::click()).clicked() {
            ctx.memory_mut(|m| m.request_focus(id));
        }

        // 서식 도구/찾기에서 예약한 편집을 TextEdit을 그리기 전에 적용
        if let Some(action) = self.pending_action.take()
            && (self.doc().is_markdown() || matches!(action, MdAction::Insert(_)))
        {
            let doc = &mut self.docs[self.active];
            let mut state = TextEditState::load(&ctx, id).unwrap_or_default();
            let end = doc.text.chars().count();
            let sel = state
                .cursor
                .char_range()
                .map(|r| {
                    let r = r.as_sorted_char_range();
                    r.start.0.min(end)..r.end.0.min(end)
                })
                .unwrap_or(end..end);
            let new_sel = markdown::apply_action(&mut doc.text, sel, &action);
            doc.mark_changed();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(new_sel.start), CCursor::new(new_sel.end))));
            state.store(&ctx, id);
            self.focus_editor = true;
            self.scroll_to_cursor = true;
        }
        if let Some(sel) = self.pending_select.take() {
            let mut state = TextEditState::load(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.start), CCursor::new(sel.end))));
            state.store(&ctx, id);
            self.scroll_to_cursor = true;
            // 찾기 카드의 "n/전체" 표시가 바로 갱신되도록
            ctx.request_repaint();
        }
        // 사이드바 등을 누른 바로 그 프레임에는 egui가 포커스를 다시 거둘 수 있어서,
        // 편집기가 실제로 포커스를 받을 때까지 몇 프레임 동안 요청합니다.
        if self.focus_editor && self.dialog.is_none() {
            if ctx.memory(|m| m.has_focus(id)) || self.focus_attempts >= 5 {
                self.focus_editor = false;
                self.focus_attempts = 0;
            } else {
                ctx.memory_mut(|m| m.request_focus(id));
                self.focus_attempts += 1;
                ctx.request_repaint();
            }
        }

        // 선택한 그림: Delete/Backspace 로 지우기, Esc 로 선택 해제
        if let Some((doc_id, line)) = self.selected_image {
            let valid = doc_id == self.doc().id && image_line_at(&self.doc().text, line).is_some();
            let typing_elsewhere = ctx.memory(|m| m.focused().is_some());
            if !valid {
                self.selected_image = None;
            } else if !typing_elsewhere && self.dialog.is_none() {
                let del = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace));
                if del {
                    self.apply_image_action(ImageAction::Delete(line));
                } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                    self.selected_image = None;
                    self.focus_editor = true;
                }
            }
        }

        let find_matches = if self.find.open && !self.find.query.is_empty() {
            self.refresh_matches();
            self.find.matches.clone()
        } else {
            Vec::new()
        };
        let find_current = self.find.current.clone();
        let mut editor_focused = false;

        let bg = c(palette.editor_bg);
        let text_color = c(palette.text);
        let accent = c(palette.accent);
        let gutter_bg = mix(bg, text_color, 0.025);
        let number_color = mix(bg, c(palette.muted), 0.75);
        let current_line_bg = mix(bg, text_color, 0.05);
        let match_bg = Color32::from_rgba_unmultiplied(palette.accent[0], palette.accent[1], palette.accent[2], 50);
        let current_match_bg = Color32::from_rgba_unmultiplied(palette.accent[0], palette.accent[1], palette.accent[2], 140);

        // 문서 모드: 기호를 드러낼 줄(커서 줄)과 그림 불러오기 상태
        let has_focus = ctx.memory(|m| m.has_focus(id));
        let active_line = if doc_mode && has_focus {
            TextEditState::load(&ctx, id).and_then(|st| st.cursor.char_range()).map(|r| {
                let text = &self.doc().text;
                let b = markdown::char_to_byte(text, r.primary.index.0);
                text[..b].matches('\n').count()
            })
        } else {
            None
        };
        let image_state = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let Some((_, _, _, uris)) = &self.doc().live_cache {
                for u in uris {
                    image_size(&ctx, u).map(|v| (v.x as i32, v.y as i32)).hash(&mut h);
                }
            }
            h.finish()
        };
        let live_style = LiveStyle {
            size: s.editor_font_size,
            body: doc_family(),
            bold: bold_family(),
            mono: egui::FontFamily::Monospace,
            text: text_color,
            muted: c(palette.muted),
            heading: c(palette.heading),
            code: c(palette.code),
            code_bg: mix(bg, text_color, 0.06),
            link: c(palette.link),
            quote: c(palette.quote),
        };

        // 그림 줄은 한 덩어리: 커서는 그림 앞/뒤에만, 그 자리에서 입력하면 새 줄을 만들어 그림 문법이 깨지지 않게
        let spans = image_spans(&self.docs[self.active]);
        let mut hide_caret = false;
        if doc_mode && has_focus {
            hide_caret = self.guard_image_edges(&ctx, id, &spans);
        }

        let last_cursor = self.last_cursor.filter(|(d, _)| *d == self.docs[self.active].id).map(|(_, c)| c);
        let mut new_cursor: Option<usize> = None;
        let selected_img = self.selected_image.filter(|(d, _)| *d == self.docs[self.active].id).map(|(_, l)| l);
        let doc = &mut self.docs[self.active];
        let base_dir = doc.dir();
        let line_count = doc.text.matches('\n').count() + 1;
        let number_font = FontId::new((s.editor_font_size * 0.75).max(8.0), egui::FontFamily::Monospace);
        let digit_w = ctx.fonts_mut(|f| f.glyph_width(&number_font, '0'));
        let gutter_w = if show_numbers { (line_count.to_string().len().max(2) as f32) * digit_w + 22.0 } else { 0.0 };

        let scroll = if wrap { ScrollArea::vertical() } else { ScrollArea::both() };
        let mut cursor_info = CursorInfo::default();
        let mut scroll_to_cursor = std::mem::take(&mut self.scroll_to_cursor);
        let mut toggle_task: Option<(usize, bool)> = None;
        let mut image_action: Option<ImageAction> = None;
        let mut image_toolbar: Option<(egui::Rect, usize, ImgAlign)> = None;

        scroll.id_salt(("editor_scroll", doc.id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(side);
                let left = ui.cursor().left();
                let bg_shape = ui.painter().add(Shape::Noop);
                ui.add_space(gutter_w);

                let ctx2 = ctx.clone();
                let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                    let text = buf.as_str();
                    let mut job = if doc_mode {
                        let key = {
                            use std::hash::{Hash, Hasher};
                            let mut h = std::collections::hash_map::DefaultHasher::new();
                            text.hash(&mut h);
                            active_line.hash(&mut h);
                            live_style.hash_into(&mut h);
                            (wrap_width as i32).hash(&mut h);
                            image_state.hash(&mut h);
                            base_dir.hash(&mut h);
                            h.finish()
                        };
                        match &doc.live_cache {
                            Some((k, job, _, _)) if *k == key => job.clone(),
                            _ => {
                                let size_of = |uri: &str| image_size(&ctx2, uri);
                                let out = live::layout(&live::LiveInput {
                                    text,
                                    style: &live_style,
                                    active_line,
                                    base_dir: base_dir.as_deref(),
                                    image_size: &size_of,
                                    max_image_width: wrap_width - 4.0,
                                });
                                doc.live_cache = Some((key, out.job.clone(), out.decos, out.image_uris));
                                out.job
                            }
                        }
                    } else {
                        let key = style.cache_key(text);
                        match &doc.highlight_cache {
                            Some((k, job)) if *k == key => job.clone(),
                            _ => {
                                let job = markdown::highlight(text, &style);
                                doc.highlight_cache = Some((key, job.clone()));
                                job
                            }
                        }
                    };
                    job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
                    ui.fonts_mut(|f| f.layout_job(job))
                };

                let row_h = s.editor_font_size * if doc_mode { 1.35 } else { s.line_spacing };
                let rows = ((ui.available_height() - 82.0) / row_h).max(1.0) as usize;
                if hide_caret {
                    // 기본 커서는 그림 줄 맨 위 구석에 그려지므로 감추고, 그림 옆에 따로 그립니다.
                    ui.visuals_mut().text_cursor.stroke.color = Color32::TRANSPARENT;
                }
                let output = TextEdit::multiline(&mut doc.text)
                    .id(id)
                    .font(if doc_mode { FontId::new(s.editor_font_size, doc_family()) } else { source_font.clone() })
                    // 프레임을 직접 주면 egui는 .margin() 대신 프레임의 안쪽 여백을 쓰므로 둘 다 지정
                    .frame(Frame::NONE.inner_margin(EDITOR_MARGIN))
                    .margin(EDITOR_MARGIN)
                    .desired_width(if wrap { col_w - gutter_w } else { f32::INFINITY })
                    .desired_rows(rows)
                    .lock_focus(true)
                    .hint_text(if doc_mode { "제목을 쓰고 Enter를 눌러 시작하세요" } else { "" })
                    .layouter(&mut layouter)
                    .show(ui);

                if output.response.response.changed() {
                    doc.mark_changed();
                }
                editor_focused = output.response.response.has_focus();

                let galley = &output.galley;
                let origin = output.galley_pos - egui::vec2(galley.rect.left(), 0.0);
                let full = output.response.response.rect;
                let clip = ui.clip_rect();
                let text_left = output.galley_pos.x;
                let text_right = full.right() - EDITOR_MARGIN.right as f32;
                let pos = |ci: usize| galley.pos_from_cursor(CCursor::new(ci)).translate(origin.to_vec2());
                let mut bg_shapes = Vec::new();
                let mut fg_shapes = Vec::new();
                let mut image_rects: Vec<(usize, usize, egui::Rect)> = Vec::new();

                if show_numbers {
                    bg_shapes.push(Shape::rect_filled(
                        egui::Rect::from_min_max(egui::pos2(left, full.top()), egui::pos2(left + gutter_w, full.bottom().max(clip.bottom()))),
                        0.0,
                        gutter_bg,
                    ));
                }

                // 문서 모드 꾸밈 (목록 점, 체크박스, 인용 막대, 구분선, 그림, 코드 블록 배경)
                if doc_mode && let Some((_, _, decos, _)) = &doc.live_cache {
                    let click = if output.response.response.clicked() { ui.input(|i| i.pointer.interact_pos()) } else { None };
                    let ctrl = ui.input(|i| i.modifiers.command);
                    let hover = ui.input(|i| i.pointer.hover_pos());
                    let muted = c(palette.muted);
                    for d in decos {
                        match d {
                            Deco::CodeBlock { start, end } => {
                                let (a, b) = (pos(*start), pos(*end));
                                let r = egui::Rect::from_min_max(egui::pos2(text_left - 10.0, a.top() - 2.0), egui::pos2(text_right + 10.0, b.bottom() + 2.0));
                                if r.intersects(clip) {
                                    bg_shapes.push(Shape::rect_filled(r, 8.0, mix(bg, text_color, 0.045)));
                                }
                            }
                            Deco::Quote { start, end } => {
                                let (a, b) = (pos(*start), pos(*end));
                                let r = egui::Rect::from_min_max(egui::pos2(text_left - 12.0, a.top() + 2.0), egui::pos2(text_left - 9.0, b.bottom() - 2.0));
                                if r.intersects(clip) {
                                    fg_shapes.push(Shape::rect_filled(r, 2.0, mix(accent, bg, 0.3)));
                                }
                            }
                            Deco::Bullet { at, level } => {
                                let (a, b) = (pos(*at), pos(*at + 1));
                                if a.intersects(clip) {
                                    let center = egui::pos2((a.left() + b.left()) / 2.0, a.center().y);
                                    let r = s.editor_font_size * 0.16;
                                    if level % 2 == 0 {
                                        fg_shapes.push(Shape::circle_filled(center, r, text_color));
                                    } else {
                                        fg_shapes.push(Shape::circle_stroke(center, r, Stroke::new(1.2, text_color)));
                                    }
                                }
                            }
                            Deco::Task { at, checked, byte } => {
                                let (a, b) = (pos(*at), pos(*at + 3));
                                let side = s.editor_font_size * 0.95;
                                let center = egui::pos2((a.left() + b.left()) / 2.0, a.center().y);
                                let r = egui::Rect::from_center_size(center, egui::vec2(side, side));
                                if r.intersects(clip) {
                                    if *checked {
                                        fg_shapes.push(Shape::rect_filled(r, 4.0, accent));
                                        fg_shapes.push(Shape::line(
                                            vec![
                                                r.left_center() + egui::vec2(side * 0.22, 0.0),
                                                r.center_bottom() + egui::vec2(-side * 0.08, -side * 0.25),
                                                r.right_top() + egui::vec2(-side * 0.2, side * 0.28),
                                            ],
                                            Stroke::new(1.8, bg),
                                        ));
                                    } else {
                                        fg_shapes.push(Shape::rect_filled(r, 4.0, bg));
                                        fg_shapes.push(Shape::rect_stroke(r, 4.0, Stroke::new(1.4, muted), egui::StrokeKind::Inside));
                                    }
                                }
                                if click.is_some_and(|p| r.expand(3.0).contains(p)) {
                                    toggle_task = Some((*byte, !checked));
                                }
                            }
                            Deco::Rule { at } => {
                                let a = pos(*at);
                                if a.intersects(clip) {
                                    let y = a.center().y;
                                    fg_shapes.push(Shape::line_segment([egui::pos2(text_left, y), egui::pos2(text_right, y)], Stroke::new(1.0, mix(bg, text_color, 0.2))));
                                }
                            }
                            Deco::Image { at, end, uri, size, top, align, line, max_w } => {
                                let a = pos(*at);
                                let x = match align {
                                    ImgAlign::Left => text_left,
                                    ImgAlign::Center => text_left + (max_w - size.x).max(0.0) / 2.0,
                                    ImgAlign::Right => text_left + (max_w - size.x).max(0.0),
                                };
                                let r = egui::Rect::from_min_size(egui::pos2(x, a.top() + top), *size);
                                image_rects.push((*at, *end, r));
                                if !r.intersects(clip) {
                                    continue;
                                }
                                if let Ok(egui::load::TexturePoll::Ready { texture }) =
                                    ctx.try_load_texture(uri, egui::TextureOptions::LINEAR, egui::SizeHint::default())
                                {
                                    fg_shapes.push(Shape::image(texture.id, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE));
                                }
                                // 그림이 있는 줄 전체를 눌러도 그림 선택 (경로 글자에 커서가 들어가지 않게)
                                let row = egui::Rect::from_x_y_ranges(text_left..=text_right, a.top()..=r.bottom() + 6.0);
                                let img_id = id.with(("image", line.start));
                                let resp = ui.interact(row, img_id, egui::Sense::click());
                                let selected = selected_img == Some(line.start);
                                if resp.clicked() {
                                    image_action = Some(ImageAction::Select(line.start));
                                }
                                if resp.hovered() || selected {
                                    let color = if selected { accent } else { mix(bg, accent, 0.5) };
                                    fg_shapes.push(Shape::rect_stroke(r, 3.0, Stroke::new(if selected { 2.0 } else { 1.0 }, color), egui::StrokeKind::Outside));
                                } else {
                                    fg_shapes.push(Shape::rect_stroke(r, 2.0, Stroke::new(1.0, mix(bg, text_color, 0.08)), egui::StrokeKind::Outside));
                                }
                                if selected {
                                    // 크기 조절 손잡이 (워드처럼 모서리를 끌어서)
                                    let corner = if *align == ImgAlign::Right { r.left_bottom() } else { r.right_bottom() };
                                    let hr = egui::Rect::from_center_size(corner, egui::vec2(18.0, 18.0));
                                    let h = ui.interact(hr, img_id.with("resize"), egui::Sense::drag());
                                    if h.hovered() || h.dragged() {
                                        ctx.set_cursor_icon(if *align == ImgAlign::Right {
                                            egui::CursorIcon::ResizeNeSw
                                        } else {
                                            egui::CursorIcon::ResizeNwSe
                                        });
                                    }
                                    fg_shapes.push(Shape::rect_filled(hr.shrink(3.0), 3.0, accent));
                                    fg_shapes.push(Shape::rect_stroke(hr.shrink(3.0), 3.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside));
                                    let current = (size.x / max_w * 100.0).round() as u32;
                                    if h.dragged()
                                        && let Some(p) = h.interact_pointer_pos()
                                    {
                                        let new_w = match align {
                                            ImgAlign::Left => p.x - r.left(),
                                            ImgAlign::Center => 2.0 * (p.x - r.center().x),
                                            ImgAlign::Right => r.right() - p.x,
                                        };
                                        let pct = (new_w / max_w * 100.0).round().clamp(5.0, 100.0) as u32;
                                        if pct != current {
                                            image_action = Some(ImageAction::Width(line.start, Some(pct)));
                                        }
                                    }
                                    image_toolbar = Some((r, line.start, *align));
                                }
                            }
                            Deco::Link { start, end, url } => {
                                let (a, b) = (pos(*start), pos(*end));
                                if (a.top() - b.top()).abs() > 1.0 {
                                    continue;
                                }
                                let r = egui::Rect::from_min_max(a.min, egui::pos2(b.left(), a.bottom()));
                                if ctrl && hover.is_some_and(|p| r.contains(p)) {
                                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if ctrl && click.is_some_and(|p| r.contains(p)) && (url.starts_with("http://") || url.starts_with("https://")) {
                                    ctx.open_url(egui::OpenUrl::new_tab(url));
                                }
                            }
                        }
                    }
                }

                // 현재 줄 강조 (찾기 칸에 포커스가 있을 때도 커서 위치를 알 수 있게 저장된 상태에서 읽음)
                let mut cursor_range = output.cursor_range.or_else(|| output.state.cursor.char_range());
                if doc_mode
                    && output.response.response.has_focus()
                    && let Some(range) = cursor_range
                {
                    let p = range.primary.index.0;
                    let vertical = ui.input(|i| {
                        [Key::ArrowUp, Key::ArrowDown, Key::PageUp, Key::PageDown].into_iter().any(|k| i.key_pressed(k))
                    });
                    // 위/아래로 들어오면 그림 앞. 그림 앞에서 → 는 그림 뒤, 그림 뒤에서 ← 는 그림 앞.
                    let snap = image_rects.iter().find_map(|&(s0, e0, _)| {
                        if s0 < p && p < e0 {
                            Some(if vertical || last_cursor != Some(s0) { s0 } else { e0 })
                        } else if p == e0 && vertical && last_cursor != Some(e0) {
                            Some(s0)
                        } else {
                            None
                        }
                    });
                    if let Some(target) = snap {
                        let snapped = if range.is_empty() {
                            CCursorRange::one(CCursor::new(target))
                        } else {
                            CCursorRange { primary: CCursor::new(target), ..range }
                        };
                        let mut st = output.state.clone();
                        st.cursor.set_char_range(Some(snapped));
                        st.store(&ctx, id);
                        cursor_range = Some(snapped);
                        ctx.request_repaint();
                    }
                }
                if let Some(range) = cursor_range {
                    new_cursor = Some(range.primary.index.0);
                }
                // 그림 앞/뒤에 있는 커서는 그림 높이만큼 세로선으로
                if doc_mode
                    && output.response.response.has_focus()
                    && let Some(range) = cursor_range
                    && range.is_empty()
                {
                    let p = range.primary.index.0;
                    if let Some(&(s0, _, r)) = image_rects.iter().find(|(s0, e0, _)| p == *s0 || p == *e0) {
                        let x = if p == s0 { r.left() - 5.0 } else { r.right() + 5.0 };
                        fg_shapes.push(Shape::line_segment([egui::pos2(x, r.top()), egui::pos2(x, r.bottom())], Stroke::new(2.0, accent)));
                        if !hide_caret {
                            ctx.request_repaint();
                        }
                    } else if hide_caret {
                        ctx.request_repaint();
                    }
                }
                let selected = cursor_range.map(|r| {
                    let r = r.as_sorted_char_range();
                    r.start.0..r.end.0
                });
                if let Some(range) = cursor_range {
                    let rect = pos(range.primary.index.0);
                    if highlight_line && range.is_empty() && output.response.response.has_focus() {
                        bg_shapes.push(Shape::rect_filled(
                            egui::Rect::from_x_y_ranges(left..=full.right().max(left + col_w), rect.y_range()),
                            0.0,
                            current_line_bg,
                        ));
                    }
                    if scroll_to_cursor {
                        ui.scroll_to_rect(rect.expand(row_h * 2.0), Some(Align::Center));
                        scroll_to_cursor = false;
                    }
                    // 줄/열 계산
                    let text = &doc.text;
                    let r = range.as_sorted_char_range();
                    let p = markdown::char_to_byte(text, range.primary.index.0);
                    let before = &text[..p];
                    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
                    cursor_info = CursorInfo {
                        line: before.matches('\n').count() + 1,
                        column: before[line_start..].chars().count() + 1,
                        selected: r.end.0 - r.start.0,
                    };
                }

                // 찾은 위치 표시
                for m in find_matches.iter().take(5000) {
                    let a = pos(m.start);
                    let b = pos(m.end);
                    if a.bottom() < clip.top() {
                        continue;
                    }
                    if a.top() > clip.bottom() {
                        break;
                    }
                    let rect = if (a.top() - b.top()).abs() < 1.0 {
                        egui::Rect::from_min_max(a.min, egui::pos2(b.max.x, a.max.y))
                    } else {
                        egui::Rect::from_min_max(a.min, egui::pos2(full.right(), a.max.y))
                    };
                    let is_current = find_current.as_ref().or(selected.as_ref()) == Some(m);
                    let color = if is_current { current_match_bg } else { match_bg };
                    bg_shapes.push(Shape::rect_filled(rect, 2.0, color));
                }

                ui.painter().set(bg_shape, Shape::Vec(bg_shapes));
                ui.painter().extend(fg_shapes);

                // 줄 번호
                if show_numbers {
                    let painter = ui.painter();
                    let mut line = 1;
                    let mut starts_line = true;
                    for row in &galley.rows {
                        let rect = row.rect().translate(origin.to_vec2());
                        if starts_line && rect.bottom() >= clip.top() {
                            if rect.top() > clip.bottom() {
                                break;
                            }
                            painter.text(
                                egui::pos2(left + gutter_w - 12.0, rect.top() + (rect.height().min(row_h * 1.6)) / 2.0),
                                Align2::RIGHT_CENTER,
                                line.to_string(),
                                number_font.clone(),
                                number_color,
                            );
                        }
                        starts_line = row.ends_with_newline;
                        if starts_line {
                            line += 1;
                        }
                    }
                }
            });
        });

        // 그림 도구 막대 (선택한 그림 위에 떠 있음)
        if let Some((r, line, align)) = image_toolbar {
            let current = image_line_at(&self.docs[self.active].text, line).and_then(|(_, w, _)| w);
            if let Some(a) = image_toolbar_ui(&ctx, r, line, align, current) {
                image_action = Some(a);
            }
        }
        match image_action {
            Some(ImageAction::Select(line)) => {
                self.selected_image = Some((self.docs[self.active].id, line));
                ctx.memory_mut(|m| m.surrender_focus(id));
                editor_focused = false;
            }
            Some(a) => self.apply_image_action(a),
            None => {
                if editor_focused {
                    self.selected_image = None;
                }
            }
        }

        if let Some((byte, checked)) = toggle_task {
            let doc = &mut self.docs[self.active];
            if matches!(doc.text.as_bytes().get(byte), Some(b' ' | b'x' | b'X')) {
                doc.text.replace_range(byte..byte + 1, if checked { "x" } else { " " });
                doc.mark_changed();
            }
        }
        self.cursor = cursor_info;
        if let Some(c) = new_cursor {
            self.last_cursor = Some((self.docs[self.active].id, c));
        }
        self.scroll_to_cursor = scroll_to_cursor;
        // 편집기로 돌아와 커서를 움직이면 다음 찾기는 그 커서에서 시작
        if editor_focused {
            self.find.current = None;
        }
    }

    /// 커서가 그림 앞/뒤에 있을 때, TextEdit보다 먼저 입력을 살펴 그림 문법이 깨지지 않게 합니다.
    /// 커서가 그림 옆에 있으면 `true` (기본 커서 대신 그림 옆에 커서를 그림).
    fn guard_image_edges(&mut self, ctx: &egui::Context, id: Id, spans: &[(usize, usize, usize)]) -> bool {
        let Some(mut state) = TextEditState::load(ctx, id) else { return false };
        let Some(range) = state.cursor.char_range() else { return false };
        if !range.is_empty() {
            return false;
        }
        let cur = range.primary.index.0;
        let Some(&(start, end, line_byte)) = spans.iter().find(|(s0, e0, _)| cur == *s0 || cur == *e0) else {
            return false;
        };
        let at_start = cur == start;
        let (typing, backspace, delete) = ctx.input(|i| {
            let typing = i.events.iter().any(|e| match e {
                egui::Event::Text(t) => !t.is_empty(),
                egui::Event::Paste(t) => !t.is_empty(),
                egui::Event::Ime(egui::ImeEvent::Preedit { text: t, .. }) | egui::Event::Ime(egui::ImeEvent::Commit(t)) => !t.is_empty(),
                egui::Event::Key { key: Key::Tab, pressed: true, .. } => true,
                _ => false,
            });
            let key = |k: Key| i.events.iter().any(|e| matches!(e, egui::Event::Key { key, pressed: true, .. } if *key == k));
            (typing, key(Key::Backspace), key(Key::Delete))
        });
        let drop_key = |k: Key| ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key, .. } if *key == k)));
        let doc = &mut self.docs[self.active];
        let mut move_to = None;

        if typing {
            // 그림 앞이면 위에, 뒤면 아래에 새 줄을 만들고 거기에 입력
            if at_start {
                doc.text.insert(markdown::char_to_byte(&doc.text, start), '\n');
                move_to = Some(start);
            } else {
                doc.text.insert(markdown::char_to_byte(&doc.text, end), '\n');
                move_to = Some(end + 1);
            }
            doc.mark_changed();
        } else if backspace && !at_start {
            // 그림 뒤에서 Backspace: 먼저 그림을 선택 (한 번 더 누르면 지움)
            drop_key(Key::Backspace);
            self.selected_image = Some((doc.id, line_byte));
            ctx.memory_mut(|m| m.surrender_focus(id));
        } else if delete && at_start {
            drop_key(Key::Delete);
            self.selected_image = Some((doc.id, line_byte));
            ctx.memory_mut(|m| m.surrender_focus(id));
        } else if backspace && at_start {
            // 앞 줄이 빈 줄이면 그 줄만 지워지므로 그대로, 아니면 지우지 않고 앞 줄 끝으로
            let b = line_byte;
            let prev_empty = b >= 1 && (b == 1 || doc.text.as_bytes()[b - 2] == b'\n');
            if b == 0 {
                drop_key(Key::Backspace);
            } else if !prev_empty {
                drop_key(Key::Backspace);
                move_to = Some(start - 1);
            }
        } else if delete && !at_start {
            // 다음 줄이 빈 줄이거나 끝이면 그대로, 아니면 다음 줄 처음으로
            let b = markdown::char_to_byte(&doc.text, end);
            let next_empty = b + 1 >= doc.text.len() || doc.text.as_bytes()[b + 1] == b'\n';
            if !next_empty {
                drop_key(Key::Delete);
                move_to = Some(end + 1);
            }
        }
        if let Some(c) = move_to {
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            state.store(ctx, id);
            return false;
        }
        self.selected_image.is_none_or(|(d, _)| d != self.docs[self.active].id)
    }

    fn apply_image_action(&mut self, action: ImageAction) {
        let doc = &mut self.docs[self.active];
        let (line, change) = match action {
            ImageAction::Select(_) => return,
            ImageAction::Width(l, w) => (l, Some((Some(w), None))),
            ImageAction::Align(l, a) => (l, Some((None, Some(a)))),
            ImageAction::Delete(l) => (l, None),
        };
        if image_line_at(&doc.text, line).is_none() {
            return;
        }
        let end = doc.text[line..].find('\n').map_or(doc.text.len(), |i| line + i);
        let body = doc.text[line..end].to_owned();
        let indent = &body[..body.len() - body.trim_start().len()];
        let Some(img) = live::parse_image_line(body.trim_start()) else { return };
        match change {
            Some((width, align)) => {
                let width = width.unwrap_or(img.width);
                let align = align.unwrap_or(img.align);
                let new_line = format!("{indent}{}", live::format_image_line(img.alt, img.dest_raw, width, align));
                if new_line != body {
                    doc.text.replace_range(line..end, &new_line);
                    doc.mark_changed();
                }
            }
            None => {
                // 줄바꿈까지 지우기
                let end = if end < doc.text.len() { end + 1 } else { end };
                doc.text.replace_range(line..end, "");
                doc.mark_changed();
                self.selected_image = None;
                self.focus_editor = true;
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else { return };
        enum Choice {
            Primary,
            Discard,
            Cancel,
        }
        let mut choice = None;
        egui::Modal::new(Id::new("dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            let (title, body, primary, discard): (&str, String, &str, Option<&str>) = match dialog {
                Dialog::CloseDoc(_) => ("저장할까요?", "이 문서의 변경 내용이 아직 파일로 저장되지 않았습니다.".into(), "저장", Some("저장 안 함")),
                Dialog::Exit => ("저장할까요?", "저장하지 않은 문서가 있습니다. 끝내기 전에 저장할까요?".into(), "모두 저장", Some("저장 안 함")),
                Dialog::Message(m) => ("알림", m.clone(), "확인", None),
                Dialog::ConfirmDelete(p) => (
                    "휴지통으로 옮길까요?",
                    format!("{}\n\n휴지통에서 다시 꺼낼 수 있습니다.", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                    "휴지통으로",
                    None,
                ),
                Dialog::NewFolder { .. } => ("새 폴더", String::new(), "만들기", None),
                Dialog::ExternalChange(id) => {
                    let name = self.docs.iter().find(|d| d.id == *id).map(doc_display_title).unwrap_or_default();
                    (
                        "파일이 밖에서 바뀌었습니다",
                        format!(
                            "{name}\n\n다른 프로그램(AI 도구, 동기화, 다른 편집기 등)이 이 파일을 고쳤는데, \
                             앱에서 고친 내용도 아직 저장되지 않았습니다. 고를 때까지 자동 저장을 멈춥니다.\n\n\
                             · 다시 불러오기: 파일의 새 내용을 엽니다 (앱에서 고친 내용은 사라집니다)\n\
                             · 내 내용 유지: 앱의 내용으로 파일을 덮어씁니다"
                        ),
                        "다시 불러오기",
                        Some("내 내용 유지"),
                    )
                }
            };
            ui.label(RichText::new(title).size(17.0).strong());
            ui.add_space(8.0);
            if let Dialog::NewFolder { name, .. } = dialog {
                let resp = ui.add(TextEdit::singleline(name).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 6)));
                if !resp.has_focus() && !resp.lost_focus() {
                    resp.request_focus();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    choice = Some(Choice::Primary);
                }
            } else {
                ui.label(body);
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let primary_btn = egui::Button::new(RichText::new(primary).color(Color32::WHITE)).fill(c(self.settings.palette().accent));
                if ui.add(primary_btn).clicked() {
                    choice = Some(Choice::Primary);
                }
                if let Some(d) = discard
                    && ui.button(d).clicked()
                {
                    choice = Some(Choice::Discard);
                }
                if !matches!(dialog, Dialog::Message(_) | Dialog::ExternalChange(_)) && ui.button("취소").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                choice = Some(Choice::Cancel);
            }
        });

        let Some(choice) = choice else { return };
        let dialog = self.dialog.take();
        match (dialog, choice) {
            (Some(Dialog::CloseDoc(id)), Choice::Primary) => {
                if let Some(i) = self.docs.iter().position(|d| d.id == id)
                    && self.save_doc(ctx, i, false)
                {
                    self.close_doc(id);
                }
            }
            (Some(Dialog::CloseDoc(id)), Choice::Discard) => self.close_doc(id),
            (Some(Dialog::Exit), Choice::Primary) => {
                if self.save_all(ctx) {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
            (Some(Dialog::Exit), Choice::Discard) => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            (Some(Dialog::ConfirmDelete(p)), Choice::Primary) => self.delete_path(ctx, &p),
            (Some(Dialog::ExternalChange(id)), Choice::Primary) => {
                if let Some(i) = self.docs.iter().position(|d| d.id == id) {
                    let old_text = self.docs[i].text.clone();
                    match self.docs[i].reload() {
                        Ok(()) => self.after_reload(ctx, i, &old_text),
                        Err(err) => self.dialog = Some(Dialog::Message(format!("다시 불러오지 못했습니다.\n\n{err}"))),
                    }
                }
            }
            (Some(Dialog::ExternalChange(id)), Choice::Discard) => {
                if let Some(d) = self.docs.iter_mut().find(|d| d.id == id) {
                    d.keep_mine();
                }
            }
            // 꼭 골라야 하는 대화상자 (Esc로 닫지 않음)
            (Some(d @ Dialog::ExternalChange(_)), Choice::Cancel) => self.dialog = Some(d),
            (Some(Dialog::NewFolder { parent, name }), Choice::Primary) => {
                let clean = sanitize_file_name(&name);
                if !clean.is_empty() {
                    let dir = parent.join(clean);
                    if let Err(err) = std::fs::create_dir(&dir) {
                        self.dialog = Some(Dialog::Message(format!("폴더를 만들지 못했습니다.\n{err}")));
                    }
                    self.sidebar.invalidate();
                }
            }
            _ => {}
        }
        self.focus_editor = self.dialog.is_none();
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("설정")
            .id(Id::new("settings_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([640.0, 470.0])
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // 왼쪽 메뉴
                    ui.vertical(|ui| {
                        ui.set_width(132.0);
                        for page in SettingsPage::ALL {
                            if nav_item(ui, page.label(), self.settings_page == page).clicked() {
                                self.settings_page = page;
                            }
                        }
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ScrollArea::vertical().id_salt("settings_scroll").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_width(ui.available_width() - 10.0);
                            match self.settings_page {
                                SettingsPage::Theme => self.settings_theme(ui),
                                SettingsPage::Font => self.settings_font(ui),
                                SettingsPage::Editor => self.settings_editor(ui),
                                SettingsPage::Keys => settings_keys(ui),
                                SettingsPage::About => settings_about(ui),
                            }
                        });
                    });
                });
            });
        self.show_settings = open;
    }

    fn settings_theme(&mut self, ui: &mut Ui) {
        ui_kit::section_title(ui, "테마");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            for preset in ThemePreset::ALL {
                let palette = Palette::preset(preset, &self.settings.custom_palette);
                if theme_card(ui, preset.label(), &palette, self.settings.theme == preset).clicked() {
                    self.settings.theme = preset;
                }
            }
        });
        ui.add_space(14.0);
        ui_kit::section_title(ui, "사용자 지정 색");
        if self.settings.theme != ThemePreset::Custom {
            ui.horizontal(|ui| {
                ui.label(RichText::new("색을 바꾸면 '사용자 지정' 테마로 바뀝니다.").small().weak());
                if ui.small_button("지금 테마에서 시작").clicked() {
                    self.settings.custom_palette = self.settings.palette();
                    self.settings.theme = ThemePreset::Custom;
                }
            });
            ui.add_space(4.0);
        }
        let mut p = self.settings.custom_palette;
        let before = p;
        ui_kit::switch_row(ui, "어두운 바탕", "글자 대비와 그림자를 어두운 화면에 맞춥니다", &mut p.dark);
        egui::Grid::new("colors").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
            let mut fields: Vec<(&str, &mut [u8; 3])> = vec![
                ("본문 배경", &mut p.editor_bg),
                ("사이드바 배경", &mut p.panel_bg),
                ("글자", &mut p.text),
                ("흐린 글자", &mut p.muted),
                ("강조 · 선택", &mut p.accent),
                ("제목", &mut p.heading),
                ("코드", &mut p.code),
                ("링크", &mut p.link),
                ("인용", &mut p.quote),
            ];
            for (i, (label, col)) in fields.iter_mut().enumerate() {
                ui.color_edit_button_srgb(col);
                ui.label(*label);
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        if p != before {
            self.settings.custom_palette = p;
            self.settings.theme = ThemePreset::Custom;
        }
    }

    fn settings_font(&mut self, ui: &mut Ui) {
        let palette = self.settings.palette();
        ui_kit::section_title(ui, "본문");
        egui::Grid::new("fonts1").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label("글꼴");
            font_picker(ui, "doc_font", &mut self.settings.doc_font, &self.fonts, &mut self.font_filter, "맑은 고딕");
            ui.end_row();
            ui.label("크기");
            ui.add(egui::Slider::new(&mut self.settings.editor_font_size, 8.0..=48.0).step_by(1.0).fixed_decimals(0).suffix(" pt"));
            ui.end_row();
        });
        ui.add_space(8.0);
        Frame::NONE
            .fill(c(palette.editor_bg))
            .stroke(Stroke::new(1.0, mix(c(palette.editor_bg), c(palette.text), 0.1)))
            .inner_margin(Margin::symmetric(14, 12))
            .corner_radius(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let size = self.settings.editor_font_size;
                ui.label(RichText::new("다람쥐 헌 쳇바퀴에 타고파").font(FontId::new(size * 1.3, bold_family())).color(c(palette.heading)));
                ui.label(RichText::new("The quick brown fox jumps over the lazy dog. 0123").font(FontId::new(size, doc_family())).color(c(palette.text)));
            });
        ui.add_space(14.0);
        ui_kit::section_title(ui, "원본 보기 (코드)");
        egui::Grid::new("fonts2").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label("글꼴");
            font_picker(ui, "editor_font", &mut self.settings.editor_font, &self.fonts, &mut self.font_filter, "기본 (고정폭)");
            ui.end_row();
            ui.label("줄 간격");
            ui.add(egui::Slider::new(&mut self.settings.line_spacing, 1.0..=2.5).step_by(0.05).fixed_decimals(2).suffix(" 배"));
            ui.end_row();
        });
        ui.add_space(14.0);
        ui_kit::section_title(ui, "화면");
        egui::Grid::new("fonts3").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label("글꼴");
            font_picker(ui, "ui_font", &mut self.settings.ui_font, &self.fonts, &mut self.font_filter, "기본 (맑은 고딕)");
            ui.end_row();
            ui.label("배율");
            let mut scale = self.settings.ui_scale;
            let resp = ui.add(egui::Slider::new(&mut scale, 0.75..=2.0).step_by(0.05).fixed_decimals(2).suffix(" 배"));
            // 끄는 동안 배율을 바꾸면 슬라이더가 손 밑에서 움직이므로 놓았을 때 적용
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                self.settings.ui_scale = scale;
            }
            ui.end_row();
        });
    }

    fn settings_editor(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        ui_kit::section_title(ui, "저장");
        ui_kit::switch_row(ui, "자동 저장", "입력을 멈추면 바로 파일에 저장합니다", &mut s.autosave);
        ui_kit::switch_row(ui, "열린 문서 기억", "저장하지 않은 새 문서까지 다음 실행 때 복원합니다", &mut s.restore_session);
        ui_kit::switch_row(
            ui,
            "그림을 문서 폴더에 함께 보관",
            "다른 곳의 그림을 넣거나 저장할 때 문서 옆 그림 폴더로 복사합니다 (백업·이동용)",
            &mut s.keep_images_with_doc,
        );
        ui.horizontal(|ui| {
            ui.label("붙여넣은 그림 폴더");
            ui.add(TextEdit::singleline(&mut s.image_folder).desired_width(110.0));
            ui.label(RichText::new("문서 기준").small().weak());
        });
        ui.add_space(12.0);
        ui_kit::section_title(ui, "보기");
        let mut source = s.view_mode == ViewMode::Source;
        if ui_kit::switch_row(ui, "마크다운 원본 보기", "기호(#, ** 등)를 그대로 보면서 편집합니다 (Ctrl+E)", &mut source) {
            s.view_mode = if source { ViewMode::Source } else { ViewMode::Document };
        }
        let mut limit = s.max_text_width > 0.0;
        if ui_kit::switch_row(ui, "본문 폭 제한", "넓은 창에서도 읽기 좋은 너비로 가운데 정렬", &mut limit) {
            s.max_text_width = if limit { 860.0 } else { 0.0 };
        }
        if limit {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.add(egui::Slider::new(&mut s.max_text_width, 500.0..=1400.0).step_by(20.0).suffix(" px"));
            });
            ui.add_space(6.0);
        }
        ui_kit::switch_row(ui, "줄 번호", "", &mut s.show_line_numbers);
        ui_kit::switch_row(ui, "서식 도구", "", &mut s.show_toolbar);
        ui_kit::switch_row(ui, "상태 표시줄", "", &mut s.show_status_bar);
        ui_kit::switch_row(ui, "사이드바", "Ctrl+\\ 로 열고 닫기", &mut s.sidebar_visible);
        ui_kit::switch_row(ui, "항상 위에 표시", "", &mut s.always_on_top);
        ui.add_space(8.0);
        ui_kit::section_title(ui, "원본 보기");
        ui_kit::switch_row(ui, "자동 줄 바꿈", "Alt+Z", &mut s.word_wrap);
        ui_kit::switch_row(ui, "현재 줄 강조", "", &mut s.highlight_current_line);
        ui_kit::switch_row(ui, "마크다운 문법 강조", "", &mut s.markdown_highlight);
        ui.add_space(16.0);
        if ui.button("모든 설정 기본값으로").clicked() {
            let keep = (std::mem::take(&mut s.recent_files), std::mem::take(&mut s.roots));
            *s = Settings { recent_files: keep.0, roots: keep.1, roots_initialized: true, ..Settings::default() };
        }
    }

    fn central(&mut self, ui: &mut Ui) -> egui::Rect {
        let bg = c(self.settings.palette().editor_bg);
        let rect = ui.available_rect_before_wrap();
        egui::CentralPanel::default().frame(Frame::NONE.fill(bg)).show(ui, |ui| self.editor(ui));
        rect
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let palette = self.settings.palette();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop")));
        let rect = ctx.content_rect();
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(120));
        painter.rect_stroke(rect.shrink(12.0), 12.0, Stroke::new(3.0, c(palette.accent)), egui::StrokeKind::Inside);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "여기에 놓으세요\n문서는 열고, 그림은 문서에 넣고, 폴더는 사이드바에 추가합니다",
            FontId::proportional(20.0),
            Color32::WHITE,
        );
    }

    fn session(&self) -> Session {
        if !self.settings.restore_session {
            return Session::default();
        }
        Session {
            docs: self
                .docs
                .iter()
                .filter(|d| !d.is_pristine() || self.docs.len() == 1)
                .map(|d| SessionDoc {
                    path: d.path.clone(),
                    text: (d.dirty || d.path.is_none()).then(|| d.text.clone()),
                    dirty: d.dirty,
                    encoding: d.encoding,
                    line_ending: d.line_ending,
                    untitled_index: d.untitled_index,
                    auto_named: d.auto_named,
                    disk: d.disk,
                })
                .collect(),
            active: self.active,
        }
    }
}

impl eframe::App for NotepadApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.settings.clamp();
        self.apply_look(&ctx);
        if self.dialog.is_none() {
            self.handle_shortcuts(&ctx);
        }
        self.handle_dropped_files(&ctx);
        // Ctrl+V 인데 클립보드에 텍스트 없이 그림만 있는 경우 (paste_hook 참고)
        if crate::paste_hook::take_request() && self.dialog.is_none() {
            let editor = editor_id(self.doc().id);
            let focus = ctx.memory(|m| m.focused());
            if focus.is_none() || focus == Some(editor) {
                self.paste_clipboard_image(&ctx);
            }
        }
        self.handle_close_request(&ctx);
        self.check_external_changes(&ctx);
        self.autosave(&ctx);
        self.show_conflict();
        self.update_title(&ctx);

        let palette = self.settings.palette();
        let panel = c(palette.panel_bg);
        let editor_bg = c(palette.editor_bg);
        if self.settings.sidebar_visible {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(250.0)
                .size_range(190.0..=460.0)
                .frame(Frame::NONE.fill(panel).inner_margin(Margin { left: 10, right: 10, top: 10, bottom: 8 }))
                .show(ui, |ui| self.sidebar_ui(ui));
        }
        egui::Panel::top("top_bar")
            .exact_size(44.0)
            .frame(Frame::NONE.fill(editor_bg).inner_margin(Margin { left: 10, right: 10, top: 8, bottom: 0 }))
            .show_separator_line(false)
            .show(ui, |ui| self.top_bar(ui));
        if self.settings.show_toolbar && self.doc().is_markdown() {
            egui::Panel::top("toolbar")
                .frame(Frame::NONE.fill(editor_bg).inner_margin(Margin { left: 8, right: 8, top: 2, bottom: 2 }))
                .show_separator_line(false)
                .show(ui, |ui| self.toolbar(ui));
        }
        if self.settings.show_status_bar {
            egui::Panel::bottom("status")
                .frame(Frame::NONE.fill(editor_bg).inner_margin(Margin { left: 14, right: 8, top: 3, bottom: 3 }))
                .show_separator_line(false)
                .show(ui, |ui| self.status_bar(ui));
        }
        let content = self.central(ui);
        if self.find.open {
            self.find_overlay(&ctx, content);
        }

        self.settings_window(&ctx);
        self.dialogs(&ctx);
        self.drop_overlay(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
        eframe::set_value(storage, "session", &self.session());
    }

    fn auto_save_interval(&self) -> std::time::Duration {
        // 비정상 종료에 대비해 자주 저장 (작은 파일이라 부담 없음)
        std::time::Duration::from_secs(10)
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let p = self.settings.palette().panel_bg;
        Color32::from_rgb(p[0], p[1], p[2]).to_normalized_gamma_f32()
    }
}

// ---------------------------------------------------------------------------
// 도우미

/// 그림에 대한 동작 (그림 줄이 시작하는 바이트 위치로 가리킴)
enum ImageAction {
    Select(usize),
    /// 본문 폭 대비 %, `None`이면 원래 크기
    Width(usize, Option<u32>),
    Align(usize, ImgAlign),
    Delete(usize),
}

/// 문서 모드에서 그림 줄들의 (시작 글자, 끝 글자, 시작 바이트)
fn image_spans(doc: &Document) -> Vec<(usize, usize, usize)> {
    let Some((_, _, decos, _)) = &doc.live_cache else { return Vec::new() };
    decos
        .iter()
        .filter_map(|d| match d {
            Deco::Image { at, end, line, .. } => Some((*at, *end, line.start)),
            _ => None,
        })
        .collect()
}

/// `line` 위치에서 시작하는 줄이 그림 줄이면 (경로, 너비, 위치)
fn image_line_at(text: &str, line: usize) -> Option<(String, Option<u32>, ImgAlign)> {
    if line > text.len() || !text.is_char_boundary(line) || (line > 0 && text.as_bytes()[line - 1] != b'\n') {
        return None;
    }
    let end = text[line..].find('\n').map_or(text.len(), |i| line + i);
    let img = live::parse_image_line(text[line..end].trim_start())?;
    Some((img.dest.to_owned(), img.width, img.align))
}

/// 선택한 그림 위에 뜨는 도구 막대: 크기 · 위치 · 지우기
fn image_toolbar_ui(ctx: &egui::Context, img: egui::Rect, line: usize, align: ImgAlign, width: Option<u32>) -> Option<ImageAction> {
    let mut action = None;
    let h = 44.0;
    let pos = if img.top() - h - 6.0 > ctx.content_rect().top() + 90.0 {
        egui::pos2(img.left(), img.top() - h - 6.0)
    } else {
        img.left_top() + egui::vec2(8.0, 8.0)
    };
    egui::Area::new(Id::new("image_toolbar")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        Frame::popup(ui.style()).inner_margin(Margin::same(5)).corner_radius(10).show(ui, |ui| {
            let base = ui.visuals().window_fill;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for pct in [25, 50, 75, 100] {
                    let label = RichText::new(format!("{pct}%")).size(12.5);
                    if ui_kit::text_button(ui, label, &format!("본문 폭의 {pct}%"), width == Some(pct), base).clicked() {
                        action = Some(ImageAction::Width(line, Some(pct)));
                    }
                }
                if ui_kit::text_button(ui, RichText::new("원래 크기").size(12.5), "그림 원래 크기 (본문 폭을 넘지 않게)", width.is_none(), base).clicked() {
                    action = Some(ImageAction::Width(line, None));
                }
                ui.add_space(8.0);
                for (a, icon, tip) in [
                    (ImgAlign::Left, Icon::AlignLeft, "왼쪽"),
                    (ImgAlign::Center, Icon::AlignCenter, "가운데"),
                    (ImgAlign::Right, Icon::AlignRight, "오른쪽"),
                ] {
                    if ui_kit::icon_button(ui, icon, tip, align == a, base).clicked() {
                        action = Some(ImageAction::Align(line, a));
                    }
                }
                ui.add_space(8.0);
                if ui_kit::icon_button(ui, Icon::Trash, "그림 지우기 (Delete)", false, base).clicked() {
                    action = Some(ImageAction::Delete(line));
                }
            });
        });
    });
    action
}

const WELCOME_NOTE: &str = "# 시작하기

TB's Notepad에 오신 것을 환영합니다. 쓰는 대로 바로 서식이 적용되는 메모장입니다.

![예시 그림](images/welcome.png \"align=center\")

## 이렇게 써 보세요

- 줄 맨 앞에 `#` 과 띄어쓰기를 쓰면 **제목**이 됩니다
- `-` 와 띄어쓰기로 목록을 만들고, `- [ ]` 로 할 일을 만듭니다
- 글자를 고르고 Ctrl+B 를 누르면 **굵게**, Ctrl+I 는 *기울임*
- 그림을 복사한 뒤 Ctrl+V 를 누르면 바로 들어갑니다
- 그림을 누르면 크기와 위치를 바꾸거나 지울 수 있습니다

## 할 일

- [x] 메모장 설치하기
- [ ] 왼쪽 사이드바의 + 로 새 메모 만들기
- [ ] 오른쪽 위 ☀/🌙 로 테마 바꿔 보기
- [ ] 위의 그림을 눌러 크기 바꿔 보기

> 메모는 이 폴더(문서\\TB Notes)에 자동으로 저장됩니다. 다른 폴더는 사이드바의 '폴더 추가'로 더할 수 있습니다.
";

/// 시작 메모에 쓰는 예시 그림 (실행 파일에 들어 있음)
const WELCOME_IMAGE: &[u8] = include_bytes!("../samples/images/banner.png");

/// `dir/시작하기.md` 와 예시 그림 `dir/images/welcome.png` 를 만듭니다.
fn write_welcome_note(dir: &Path) -> std::io::Result<()> {
    let images = dir.join("images");
    std::fs::create_dir_all(&images)?;
    let img = images.join("welcome.png");
    if !img.exists() {
        std::fs::write(&img, WELCOME_IMAGE)?;
    }
    std::fs::write(dir.join("시작하기.md"), WELCOME_NOTE)
}

fn editor_id(doc_id: u64) -> Id {
    Id::new(("editor", doc_id))
}

/// 불러온 그림의 크기 (아직이거나 실패하면 `None`)
fn image_size(ctx: &egui::Context, uri: &str) -> Option<egui::Vec2> {
    match ctx.try_load_texture(uri, egui::TextureOptions::LINEAR, egui::SizeHint::default()) {
        Ok(egui::load::TexturePoll::Ready { texture }) => Some(texture.size),
        _ => None,
    }
}

/// 문서 첫 줄에서 제목을 뽑습니다 (파일 이름용)
fn title_from_text(text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let cleaned: String = line.trim().trim_start_matches('#').replace(['*', '_', '`', '~', '[', ']'], "");
    let name = sanitize_file_name(cleaned.trim());
    (!name.is_empty()).then_some(name)
}

fn doc_display_title(doc: &Document) -> String {
    match &doc.path {
        Some(p) if !doc.is_markdown() => doc.title(),
        Some(p) => p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| doc.title()),
        None => title_from_text(&doc.text).unwrap_or_else(|| doc.title()),
    }
}

fn is_markdown_path(path: &Path) -> bool {
    has_extension(path, &["md", "markdown", "mdown", "mkd", "mdx"])
}

/// `dir/이름.ext`, 이미 있으면 `dir/이름 2.ext` …
fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem} {n}.{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// 메뉴 항목 (오른쪽에 단축키 표시). 누르면 메뉴를 닫고 `true`.
fn item(ui: &mut Ui, label: &str, shortcut: &str) -> bool {
    let mut btn = egui::Button::new(label);
    if !shortcut.is_empty() {
        btn = btn.shortcut_text(shortcut);
    }
    let clicked = ui.add(btn).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn font_picker(ui: &mut Ui, id: &str, current: &mut Option<String>, fonts: &FontLibrary, filter: &mut String, default_label: &str) {
    let selected = current.clone().unwrap_or_else(|| default_label.to_owned());
    egui::ComboBox::from_id_salt(id).selected_text(selected).width(240.0).height(360.0).show_ui(ui, |ui| {
        ui.add(TextEdit::singleline(filter).hint_text("글꼴 검색").desired_width(f32::INFINITY));
        ui.separator();
        ui.selectable_value(current, None, default_label);
        let f = filter.to_lowercase();
        for font in fonts.fonts.iter().filter(|x| f.is_empty() || x.name.to_lowercase().contains(&f)) {
            ui.selectable_value(current, Some(font.name.clone()), &font.name);
        }
    });
}

fn has_extension(path: &Path, exts: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e.to_lowercase().as_str()))
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.canonicalize().map(|a| a == b).unwrap_or(false)
}

fn sanitize_file_name(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| !r#"\/:*?"<>|"#.contains(*c) && !c.is_control()).take(60).collect();
    cleaned.trim().trim_end_matches('.').trim().to_owned()
}

fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 설정 등이 저장되는 폴더
pub fn app_data_dir() -> PathBuf {
    crate::data_dir()
}

fn open_in_file_manager(dir: &Path) -> std::io::Result<()> {
    let cmd = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(cmd).arg(dir).spawn().map(|_| ())
}

/// 파일이면 탐색기에서 그 파일을 선택한 채로, 폴더면 그 폴더를 엽니다.
fn reveal_in_file_manager(path: &Path) -> std::io::Result<()> {
    if cfg!(windows) && path.is_file() {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        return std::process::Command::new("explorer").arg(arg).spawn().map(|_| ());
    }
    open_in_file_manager(if path.is_dir() { path } else { path.parent().unwrap_or(path) })
}

/// 설정 창 왼쪽 메뉴 한 줄
fn nav_item(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let k = ui_kit::colors(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    let fill = if selected {
        mix(k.panel, k.accent, 0.14)
    } else if resp.hovered() {
        mix(k.panel, k.text, 0.06)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let color = if selected { k.accent } else { k.text };
    ui.painter().text(egui::pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), color);
    resp
}

/// 테마 미리보기 카드
fn theme_card(ui: &mut Ui, name: &str, p: &Palette, selected: bool) -> egui::Response {
    let k = ui_kit::colors(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(138.0, 104.0), egui::Sense::click());
    let painter = ui.painter();
    let preview = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 76.0));
    painter.rect_filled(preview, 10.0, c(p.editor_bg));
    // 왼쪽 사이드바 띠
    let side = egui::Rect::from_min_size(preview.min, egui::vec2(34.0, preview.height()));
    painter.rect_filled(side, egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, c(p.panel_bg));
    for i in 0..3 {
        let y = preview.top() + 14.0 + i as f32 * 10.0;
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(preview.left() + 7.0, y), egui::vec2(20.0, 4.0)), 2.0, mix(c(p.panel_bg), c(p.text), 0.35));
    }
    let x = preview.left() + 44.0;
    let bar = |y: f32, w: f32, h: f32, col: Color32| {
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x, preview.top() + y), egui::vec2(w, h)), h / 2.0, col);
    };
    bar(14.0, 56.0, 7.0, c(p.heading));
    bar(28.0, 80.0, 4.0, mix(c(p.editor_bg), c(p.text), 0.75));
    bar(37.0, 66.0, 4.0, mix(c(p.editor_bg), c(p.text), 0.75));
    bar(50.0, 28.0, 6.0, c(p.accent));
    painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x + 34.0, preview.top() + 50.0), egui::vec2(24.0, 6.0)), 3.0, c(p.code));
    let border = if selected {
        Stroke::new(2.0, k.accent)
    } else if resp.hovered() {
        Stroke::new(1.0, mix(k.panel, k.text, 0.35))
    } else {
        Stroke::new(1.0, mix(k.panel, k.text, 0.12))
    };
    painter.rect_stroke(preview, 10.0, border, egui::StrokeKind::Inside);
    let label_color = if selected { k.accent } else { k.text };
    painter.text(egui::pos2(rect.center().x, preview.bottom() + 15.0), Align2::CENTER_CENTER, name, FontId::proportional(12.5), label_color);
    resp
}

fn settings_keys(ui: &mut Ui) {
    ui_kit::section_title(ui, "단축키");
    egui::Grid::new("keys").num_columns(2).spacing([28.0, 8.0]).show(ui, |ui| {
        for (k, d) in [
            ("Ctrl+N", "새 메모"),
            ("Ctrl+O / S", "파일 열기 / 저장"),
            ("Ctrl+P", "메모 검색"),
            ("Ctrl+\\", "사이드바 열고 닫기"),
            ("F2", "지금 메모 이름 바꾸기"),
            ("Ctrl+W", "문서 닫기"),
            ("Ctrl+F / H", "찾기 / 바꾸기"),
            ("Ctrl+Z / Ctrl+Y", "실행 취소 / 다시 실행"),
            ("Ctrl+V", "붙여넣기 (복사한 그림도)"),
            ("Ctrl+B / I / K", "굵게 / 기울임 / 링크"),
            ("Ctrl+Alt+1~3", "제목 1~3"),
            ("Ctrl+클릭", "링크 열기"),
            ("그림 클릭", "크기·위치 도구, 모서리 끌어 크기 조절, Delete 로 지우기"),
            ("Ctrl+E", "마크다운 원본 보기"),
            ("F5", "날짜/시간 넣기"),
            ("Ctrl+휠 · Ctrl+= / -", "글자 크기"),
            ("Ctrl+,", "설정"),
            ("F11", "전체 화면"),
        ] {
            ui.label(RichText::new(k).monospace().size(12.5));
            ui.label(d);
            ui.end_row();
        }
    });
    ui.add_space(10.0);
    ui.label(RichText::new("파일을 창에 끌어다 놓으면 열리고, 그림은 문서에, 폴더는 사이드바에 들어갑니다.").small().weak());
}

fn settings_about(ui: &mut Ui) {
    ui.add_space(8.0);
    ui.label(RichText::new(APP_NAME).size(22.0).strong());
    ui.label(RichText::new(format!("버전 {}", env!("CARGO_PKG_VERSION"))).weak());
    ui.add_space(8.0);
    ui.label("Rust와 egui로 만든 가볍고 쉬운 메모장");
    ui.add_space(16.0);
    ui_kit::section_title(ui, "설정 저장 위치");
    ui.horizontal(|ui| {
        ui.label(RichText::new(app_data_dir().display().to_string()).small());
        if ui.small_button("열기").clicked() {
            let _ = std::fs::create_dir_all(app_data_dir());
            let _ = open_in_file_manager(&app_data_dir());
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new("실행 파일 옆에 portable.txt 파일을 만들어 두면 설정을 실행 파일 옆 data 폴더에 저장합니다 (USB 휴대용).")
            .small()
            .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_first_line() {
        assert_eq!(title_from_text("\n\n# 장보기: 우유/빵\n내용"), Some("장보기 우유빵".to_owned()));
        assert_eq!(title_from_text("   \n"), None);
        assert_eq!(title_from_text("**중요** 회의"), Some("중요 회의".to_owned()));
    }
}
