//! 메인 앱: 사이드바(폴더 트리), 문서 편집(서식이 바로 보이는 문서 모드 / 원본 보기),
//! 찾기, 설정, 자동 저장, 세션 복원.

mod chrome;
mod decorations;
mod dialogs;
mod editor;
mod files;
mod find;
mod images;
mod input;
mod media_cards;
mod note_links;
mod settings_ui;
mod slash;

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

use files::*;
use images::*;

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
    NewFolder {
        parent: PathBuf,
        name: String,
    },
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
    const ALL: [SettingsPage; 5] =
        [SettingsPage::Theme, SettingsPage::Font, SettingsPage::Editor, SettingsPage::Keys, SettingsPage::About];

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
    /// 지난 프레임에 기호를 드러낸 줄: (문서 id, 줄)
    revealed_line: Option<(u64, Option<usize>)>,
    scroll_to_cursor: bool,
    focus_editor: bool,
    focus_attempts: u8,
    cursor: CursorInfo,
    status: Option<(String, f64)>,
    last_title: String,
    allow_close: bool,
    /// 마지막으로 파일 변경을 확인한 때 (`ctx.input(|i| i.time)`)
    last_disk_check: f64,
    /// 소리 재생기, 소리·영상 파일 정보
    media: media_cards::MediaHub,
    /// 메모 링크 고르기 창
    link_picker: Option<note_links::LinkPicker>,
    /// `/` 명령 목록
    slash: Option<slash::SlashMenu>,
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
            revealed_line: None,
            scroll_to_cursor: false,
            focus_editor: true,
            focus_attempts: 0,
            cursor: CursorInfo::default(),
            status: None,
            last_title: String::new(),
            allow_close: false,
            last_disk_check: 0.0,
            media: media_cards::MediaHub::default(),
            link_picker: None,
            slash: None,
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
        let fonts_changed = prev.as_ref().is_none_or(|p| {
            p.editor_font != look.editor_font || p.ui_font != look.ui_font || p.doc_font != look.doc_font
        });
        if fonts_changed {
            ctx.set_fonts(self.fonts.definitions(
                look.editor_font.as_deref(),
                look.ui_font.as_deref(),
                look.doc_font.as_deref(),
            ));
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
        if self.dialog.is_none() && self.link_picker.is_none() {
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
        self.media.poll(&ctx, self.settings.audio_volume);
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
        self.link_picker_ui(&ctx);
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

pub(super) fn editor_id(doc_id: u64) -> Id {
    Id::new(("editor", doc_id))
}

/// 설정 등이 저장되는 폴더
pub fn app_data_dir() -> PathBuf {
    crate::data_dir()
}
