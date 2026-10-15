//! 사용자 설정과 색 테마.

use std::path::PathBuf;

use egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    /// 마크다운 기호를 그대로 보여 주는 편집 (고급)
    Source,
    /// 서식이 바로 보이는 문서 편집 (기본).
    /// 예전 버전의 "Edit/Split/Preview" 값도 여기로 읽혀 설정이 깨지지 않습니다.
    #[serde(other)]
    Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemePreset {
    Light,
    Dark,
    Sepia,
    Nord,
    Dracula,
    SolarizedLight,
    SolarizedDark,
    Forest,
    Custom,
}

impl ThemePreset {
    pub const ALL: [ThemePreset; 9] = [
        ThemePreset::Light,
        ThemePreset::Dark,
        ThemePreset::Sepia,
        ThemePreset::Nord,
        ThemePreset::Dracula,
        ThemePreset::SolarizedLight,
        ThemePreset::SolarizedDark,
        ThemePreset::Forest,
        ThemePreset::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemePreset::Light => "라이트",
            ThemePreset::Dark => "다크",
            ThemePreset::Sepia => "세피아 (종이)",
            ThemePreset::Nord => "노드",
            ThemePreset::Dracula => "드라큘라",
            ThemePreset::SolarizedLight => "솔라라이즈드 라이트",
            ThemePreset::SolarizedDark => "솔라라이즈드 다크",
            ThemePreset::Forest => "숲",
            ThemePreset::Custom => "사용자 지정",
        }
    }
}

/// 테마 하나를 이루는 색들. `[r, g, b]`로 저장합니다.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub dark: bool,
    pub editor_bg: [u8; 3],
    pub panel_bg: [u8; 3],
    pub text: [u8; 3],
    pub muted: [u8; 3],
    pub accent: [u8; 3],
    pub heading: [u8; 3],
    pub code: [u8; 3],
    pub link: [u8; 3],
    pub quote: [u8; 3],
}

const fn hex(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

pub fn c(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

impl Palette {
    pub fn preset(preset: ThemePreset, custom: &Palette) -> Palette {
        match preset {
            ThemePreset::Light => Palette {
                dark: false,
                editor_bg: hex(0xFFFFFF),
                panel_bg: hex(0xF3F3F3),
                text: hex(0x1F1F1F),
                muted: hex(0x8A8A8A),
                accent: hex(0x2F6FEB),
                heading: hex(0x0B5CAD),
                code: hex(0xB3261E),
                link: hex(0x1A73E8),
                quote: hex(0x5F6B7A),
            },
            ThemePreset::Dark => Palette {
                dark: true,
                editor_bg: hex(0x1E1E1E),
                panel_bg: hex(0x252526),
                text: hex(0xD4D4D4),
                muted: hex(0x808080),
                accent: hex(0x3794FF),
                heading: hex(0x569CD6),
                code: hex(0xCE9178),
                link: hex(0x4FC1FF),
                quote: hex(0x6A9955),
            },
            ThemePreset::Sepia => Palette {
                dark: false,
                editor_bg: hex(0xF4ECD8),
                panel_bg: hex(0xEADFC5),
                text: hex(0x5B4636),
                muted: hex(0x9C8A73),
                accent: hex(0xA0522D),
                heading: hex(0x8B4513),
                code: hex(0x8E3B46),
                link: hex(0x2E6F8E),
                quote: hex(0x7A6A55),
            },
            ThemePreset::Nord => Palette {
                dark: true,
                editor_bg: hex(0x2E3440),
                panel_bg: hex(0x3B4252),
                text: hex(0xD8DEE9),
                muted: hex(0x7B88A1),
                accent: hex(0x88C0D0),
                heading: hex(0x81A1C1),
                code: hex(0xA3BE8C),
                link: hex(0x88C0D0),
                quote: hex(0xB48EAD),
            },
            ThemePreset::Dracula => Palette {
                dark: true,
                editor_bg: hex(0x282A36),
                panel_bg: hex(0x21222C),
                text: hex(0xF8F8F2),
                muted: hex(0x6272A4),
                accent: hex(0xBD93F9),
                heading: hex(0xFF79C6),
                code: hex(0x50FA7B),
                link: hex(0x8BE9FD),
                quote: hex(0xF1FA8C),
            },
            ThemePreset::SolarizedLight => Palette {
                dark: false,
                editor_bg: hex(0xFDF6E3),
                panel_bg: hex(0xEEE8D5),
                text: hex(0x586E75),
                muted: hex(0x93A1A1),
                accent: hex(0x268BD2),
                heading: hex(0xCB4B16),
                code: hex(0x859900),
                link: hex(0x268BD2),
                quote: hex(0x6C71C4),
            },
            ThemePreset::SolarizedDark => Palette {
                dark: true,
                editor_bg: hex(0x002B36),
                panel_bg: hex(0x073642),
                text: hex(0x93A1A1),
                muted: hex(0x586E75),
                accent: hex(0x268BD2),
                heading: hex(0xB58900),
                code: hex(0x859900),
                link: hex(0x2AA198),
                quote: hex(0x6C71C4),
            },
            ThemePreset::Forest => Palette {
                dark: true,
                editor_bg: hex(0x1B2421),
                panel_bg: hex(0x222D29),
                text: hex(0xD5E3DC),
                muted: hex(0x7E978C),
                accent: hex(0x5FD3A5),
                heading: hex(0x8FE0B8),
                code: hex(0xE6C07B),
                link: hex(0x6CC4E8),
                quote: hex(0xA3B8AE),
            },
            ThemePreset::Custom => *custom,
        }
    }

    /// 화면 전체(egui 위젯)에 팔레트를 적용합니다.
    pub fn apply(&self, ctx: &egui::Context) {
        use egui::{CornerRadius, FontId, Shadow, Stroke, TextStyle};

        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        let bg = c(self.editor_bg);
        let panel = c(self.panel_bg);
        let text = c(self.text);
        let accent = c(self.accent);
        let hairline = mix(panel, text, 0.10);

        v.panel_fill = panel;
        v.window_fill = mix(panel, bg, 0.5);
        v.extreme_bg_color = bg;
        v.text_edit_bg_color = Some(mix(bg, text, 0.035));
        v.faint_bg_color = mix(bg, text, 0.04);
        v.code_bg_color = mix(bg, text, 0.08);
        v.override_text_color = Some(text);
        v.weak_text_color = Some(mix(text, panel, 0.45));
        v.hyperlink_color = c(self.link);
        v.selection.bg_fill = mix(bg, accent, 0.30);
        v.selection.stroke = Stroke::new(1.0, accent);
        v.text_cursor.stroke = Stroke::new(2.0, accent);

        // 둥글고 납작하게: 테두리 없이 은은한 배경으로만 상태 표시
        let r = CornerRadius::same(7);
        for (w, fill) in [
            (&mut v.widgets.noninteractive, panel),
            (&mut v.widgets.inactive, mix(panel, text, 0.07)),
            (&mut v.widgets.hovered, mix(panel, text, 0.12)),
            (&mut v.widgets.active, mix(panel, accent, 0.30)),
            (&mut v.widgets.open, mix(panel, text, 0.10)),
        ] {
            w.corner_radius = r;
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke.color = text;
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, hairline); // 구분선
        v.widgets.inactive.fg_stroke.color = mix(text, panel, 0.15);
        v.widgets.active.fg_stroke.color = text;

        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.window_stroke = Stroke::new(1.0, hairline);
        let shadow_alpha = if self.dark { 110 } else { 38 };
        v.window_shadow = Shadow { offset: [0, 10], blur: 32, spread: 0, color: egui::Color32::from_black_alpha(shadow_alpha) };
        v.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: egui::Color32::from_black_alpha(shadow_alpha) };
        v.striped = false;
        v.indent_has_left_vline = false;
        v.collapsing_header_frame = false;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;

        ctx.set_theme(if self.dark { egui::Theme::Dark } else { egui::Theme::Light });
        ctx.all_styles_mut(|style| {
            style.visuals = v.clone();
            let t = &mut style.text_styles;
            t.insert(TextStyle::Body, FontId::proportional(14.0));
            t.insert(TextStyle::Button, FontId::proportional(14.0));
            t.insert(TextStyle::Small, FontId::proportional(12.0));
            t.insert(TextStyle::Heading, FontId::proportional(20.0));
            t.insert(TextStyle::Monospace, FontId::monospace(13.0));
            let sp = &mut style.spacing;
            sp.item_spacing = egui::vec2(6.0, 6.0);
            sp.button_padding = egui::vec2(10.0, 5.0);
            sp.interact_size = egui::vec2(28.0, 28.0);
            sp.menu_margin = egui::Margin::same(6);
            sp.window_margin = egui::Margin::same(16);
            sp.menu_width = 240.0;
            sp.combo_height = 320.0;
            sp.slider_width = 180.0;
            sp.scroll.floating = true;
            sp.scroll.bar_width = 8.0;
        });
    }
}

/// 두 색을 `t` 비율로 섞습니다 (0 = a, 1 = b).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePreset,
    pub custom_palette: Palette,

    /// 문서 모드 본문 글꼴. `None`이면 맑은 고딕.
    pub doc_font: Option<String>,
    /// 원본 보기(코드) 글꼴. `None`이면 기본 고정폭 글꼴.
    pub editor_font: Option<String>,
    /// 메뉴 등 화면 글꼴. `None`이면 기본 글꼴.
    pub ui_font: Option<String>,
    pub editor_font_size: f32,
    /// 줄 간격 배율 (1.0 = 기본)
    pub line_spacing: f32,
    /// 화면 전체 배율
    pub ui_scale: f32,

    pub word_wrap: bool,
    pub show_line_numbers: bool,
    pub highlight_current_line: bool,
    pub markdown_highlight: bool,
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    /// 본문을 가운데 두고 이 너비(pt)를 넘지 않게 합니다. 0이면 창 너비 전체.
    pub max_text_width: f32,
    pub view_mode: ViewMode,
    pub always_on_top: bool,
    /// 종료할 때 열린 탭(저장 안 한 내용 포함)을 기억했다가 다음에 복원
    pub restore_session: bool,
    /// 붙여넣은 이미지를 문서 옆 어느 폴더에 둘지
    pub image_folder: String,
    /// 다른 곳의 그림을 넣거나 문서를 저장할 때 그림을 문서 폴더에 복사해 함께 보관
    pub keep_images_with_doc: bool,

    pub recent_files: Vec<PathBuf>,

    /// 사이드바에 보여 줄 메모 폴더들
    pub roots: Vec<PathBuf>,
    /// 처음 실행할 때 기본 메모 폴더를 한 번만 만들기 위한 표시
    pub roots_initialized: bool,
    pub sidebar_visible: bool,
    /// 파일로 저장된 메모는 입력을 멈추면 바로 저장
    pub autosave: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePreset::Light,
            custom_palette: Palette::preset(ThemePreset::Sepia, &DUMMY),
            doc_font: None,
            editor_font: None,
            ui_font: None,
            editor_font_size: 16.0,
            line_spacing: 1.25,
            ui_scale: 1.0,
            word_wrap: true,
            show_line_numbers: false,
            highlight_current_line: true,
            markdown_highlight: true,
            show_toolbar: true,
            show_status_bar: true,
            max_text_width: 860.0,
            view_mode: ViewMode::Document,
            always_on_top: false,
            restore_session: true,
            image_folder: "images".to_owned(),
            keep_images_with_doc: true,
            recent_files: Vec::new(),
            roots: Vec::new(),
            roots_initialized: false,
            sidebar_visible: true,
            autosave: true,
        }
    }
}

const DUMMY: Palette = Palette {
    dark: false,
    editor_bg: [255; 3],
    panel_bg: [255; 3],
    text: [0; 3],
    muted: [128; 3],
    accent: [0, 0, 255],
    heading: [0; 3],
    code: [0; 3],
    link: [0, 0, 255],
    quote: [0; 3],
};

impl Settings {
    pub fn palette(&self) -> Palette {
        Palette::preset(self.theme, &self.custom_palette)
    }

    pub fn push_recent(&mut self, path: PathBuf) {
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(12);
    }

    pub fn clamp(&mut self) {
        self.editor_font_size = self.editor_font_size.clamp(6.0, 96.0);
        self.line_spacing = self.line_spacing.clamp(1.0, 3.0);
        self.ui_scale = self.ui_scale.clamp(0.6, 2.5);
        if self.max_text_width != 0.0 {
            self.max_text_width = self.max_text_width.clamp(400.0, 2000.0);
        }
    }
}
