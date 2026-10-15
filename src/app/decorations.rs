//! 문서 모드 꾸밈 그리기: 코드 블록 배경, 인용 막대, 목록 점, 체크박스, 구분선,
//! 그림(선택 테두리·크기 손잡이), 소리·영상 카드, Ctrl+클릭 링크(웹 주소는 브라우저, 나머지는 `note_links.rs`).
//!
//! 무엇을 어디에 그릴지는 `live.rs`가 [`Deco`]로 알려 주고, 여기서는 배치된 글자 위치에 맞춰 그립니다.

use super::media_cards::{MediaAction, MediaHub, audio_card, media_path, video_card};
use super::*;
use crate::media::MediaKind;

/// 꾸밈을 그릴 때 필요한 편집기 상태
pub(super) struct DecoView<'a> {
    pub galley: &'a egui::Galley,
    /// 배치된 글자 좌표를 화면 좌표로 옮기는 양
    pub origin: egui::Vec2,
    pub clip: egui::Rect,
    pub text_left: f32,
    pub text_right: f32,
    pub font_size: f32,
    pub editor_id: Id,
    /// 선택한 그림 줄의 시작 바이트
    pub selected_image: Option<usize>,
    /// 이번 프레임에 편집기를 누른 위치
    pub clicked_at: Option<egui::Pos2>,
    pub bg: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub media: &'a MediaHub,
}

/// 그린 결과와 사용자가 한 일
#[derive(Default)]
pub(super) struct DecoOutput {
    /// 글자 아래에 깔 모양 (코드 블록 배경)
    pub bg: Vec<Shape>,
    /// 글자 위에 그릴 모양
    pub fg: Vec<Shape>,
    /// 그림 줄: (시작 글자, 끝 글자, 화면 위치)
    pub image_rects: Vec<(usize, usize, egui::Rect)>,
    /// 누른 체크박스: (원문 바이트, 새 상태)
    pub toggle_task: Option<(usize, bool)>,
    pub image_action: Option<ImageAction>,
    /// 도구 막대를 띄울 선택한 그림: (화면 위치, 줄 시작 바이트, 정렬, 종류)
    pub image_toolbar: Option<(egui::Rect, usize, ImgAlign, MediaKind)>,
    pub media_action: Option<MediaAction>,
    /// 길이·첫 장면을 아직 모르는 소리·영상 파일
    pub media_requests: Vec<PathBuf>,
    /// Ctrl+클릭한 웹 주소가 아닌 링크 (메모 · 폴더 · 줄 · 이 PC의 파일)
    pub follow_link: Option<String>,
}

impl DecoView<'_> {
    fn pos(&self, ci: usize) -> egui::Rect {
        self.galley.pos_from_cursor(CCursor::new(ci)).translate(self.origin)
    }
}

pub(super) fn paint_decorations(ui: &mut Ui, view: &DecoView<'_>, decos: &[Deco]) -> DecoOutput {
    let mut out = DecoOutput::default();
    for d in decos {
        match d {
            Deco::CodeBlock { start, end } => code_block(view, *start, *end, &mut out),
            Deco::Quote { start, end } => quote(view, *start, *end, &mut out),
            Deco::Bullet { at, level } => bullet(view, *at, *level, &mut out),
            Deco::Task { at, checked, byte } => task(view, *at, *checked, *byte, &mut out),
            Deco::Rule { at } => rule(view, *at, &mut out),
            Deco::Image { kind, at, end, uri, size, top, align, line, max_w } => {
                let img = ImageDeco {
                    kind: *kind,
                    at: *at,
                    end: *end,
                    uri,
                    size: *size,
                    top: *top,
                    align: *align,
                    line_start: line.start,
                    max_w: *max_w,
                };
                image(ui, view, &img, &mut out);
            }
            Deco::Link { start, end, url } => link(ui, view, *start, *end, url, &mut out),
        }
    }
    out
}

fn code_block(v: &DecoView<'_>, start: usize, end: usize, out: &mut DecoOutput) {
    let (a, b) = (v.pos(start), v.pos(end));
    let r = egui::Rect::from_min_max(
        egui::pos2(v.text_left - 10.0, a.top() - 2.0),
        egui::pos2(v.text_right + 10.0, b.bottom() + 2.0),
    );
    if r.intersects(v.clip) {
        out.bg.push(Shape::rect_filled(r, 8.0, mix(v.bg, v.text, 0.045)));
    }
}

fn quote(v: &DecoView<'_>, start: usize, end: usize, out: &mut DecoOutput) {
    let (a, b) = (v.pos(start), v.pos(end));
    let r = egui::Rect::from_min_max(
        egui::pos2(v.text_left - 12.0, a.top() + 2.0),
        egui::pos2(v.text_left - 9.0, b.bottom() - 2.0),
    );
    if r.intersects(v.clip) {
        out.fg.push(Shape::rect_filled(r, 2.0, mix(v.accent, v.bg, 0.3)));
    }
}

fn bullet(v: &DecoView<'_>, at: usize, level: usize, out: &mut DecoOutput) {
    let (a, b) = (v.pos(at), v.pos(at + 1));
    if !a.intersects(v.clip) {
        return;
    }
    let center = egui::pos2((a.left() + b.left()) / 2.0, a.center().y);
    let r = v.font_size * 0.16;
    // 단계마다 꽉 찬 점 / 빈 점을 번갈아
    out.fg.push(if level.is_multiple_of(2) {
        Shape::circle_filled(center, r, v.text)
    } else {
        Shape::circle_stroke(center, r, Stroke::new(1.2, v.text))
    });
}

fn task(v: &DecoView<'_>, at: usize, checked: bool, byte: usize, out: &mut DecoOutput) {
    let (a, b) = (v.pos(at), v.pos(at + 3));
    let side = v.font_size * 0.95;
    let center = egui::pos2((a.left() + b.left()) / 2.0, a.center().y);
    let r = egui::Rect::from_center_size(center, egui::vec2(side, side));
    if r.intersects(v.clip) {
        if checked {
            out.fg.push(Shape::rect_filled(r, 4.0, v.accent));
            out.fg.push(Shape::line(
                vec![
                    r.left_center() + egui::vec2(side * 0.22, 0.0),
                    r.center_bottom() + egui::vec2(-side * 0.08, -side * 0.25),
                    r.right_top() + egui::vec2(-side * 0.2, side * 0.28),
                ],
                Stroke::new(1.8, v.bg),
            ));
        } else {
            out.fg.push(Shape::rect_filled(r, 4.0, v.bg));
            out.fg.push(Shape::rect_stroke(r, 4.0, Stroke::new(1.4, v.muted), egui::StrokeKind::Inside));
        }
    }
    if v.clicked_at.is_some_and(|p| r.expand(3.0).contains(p)) {
        out.toggle_task = Some((byte, !checked));
    }
}

fn rule(v: &DecoView<'_>, at: usize, out: &mut DecoOutput) {
    let a = v.pos(at);
    if a.intersects(v.clip) {
        let y = a.center().y;
        out.fg.push(Shape::line_segment(
            [egui::pos2(v.text_left, y), egui::pos2(v.text_right, y)],
            Stroke::new(1.0, mix(v.bg, v.text, 0.2)),
        ));
    }
}

struct ImageDeco<'a> {
    kind: MediaKind,
    at: usize,
    end: usize,
    uri: &'a str,
    size: egui::Vec2,
    top: f32,
    align: ImgAlign,
    line_start: usize,
    max_w: f32,
}

fn image(ui: &mut Ui, v: &DecoView<'_>, img: &ImageDeco<'_>, out: &mut DecoOutput) {
    let a = v.pos(img.at);
    let x = match img.align {
        ImgAlign::Left => v.text_left,
        ImgAlign::Center => v.text_left + (img.max_w - img.size.x).max(0.0) / 2.0,
        ImgAlign::Right => v.text_left + (img.max_w - img.size.x).max(0.0),
    };
    let r = egui::Rect::from_min_size(egui::pos2(x, a.top() + img.top), img.size);
    out.image_rects.push((img.at, img.end, r));
    if !r.intersects(v.clip) {
        return;
    }
    let ctx = ui.ctx().clone();
    if img.kind == MediaKind::Image
        && let Ok(egui::load::TexturePoll::Ready { texture }) =
            ctx.try_load_texture(img.uri, egui::TextureOptions::LINEAR, egui::SizeHint::default())
    {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        out.fg.push(Shape::image(texture.id, r, uv, Color32::WHITE));
    }

    // 그림이 있는 줄 전체를 눌러도 그림 선택 (경로 글자에 커서가 들어가지 않게)
    let row = egui::Rect::from_x_y_ranges(v.text_left..=v.text_right, a.top()..=r.bottom() + 6.0);
    let img_id = v.editor_id.with(("image", img.line_start));
    let resp = ui.interact(row, img_id, egui::Sense::click());
    let selected = v.selected_image == Some(img.line_start);
    if resp.clicked() {
        out.image_action = Some(ImageAction::Select(img.line_start));
    }
    // 소리·영상 카드 (재생 버튼은 줄 선택보다 나중에 등록해서 먼저 눌리게)
    if let Some(path) = media_path(img.kind, img.uri) {
        match img.kind {
            MediaKind::Audio => audio_card(ui, v, r, &path, img_id, out),
            _ => video_card(ui, v, r, &path, img_id, &resp, out),
        }
    }
    let border = if selected {
        Stroke::new(2.0, v.accent)
    } else if resp.hovered() {
        Stroke::new(1.0, mix(v.bg, v.accent, 0.5))
    } else {
        Stroke::new(1.0, mix(v.bg, v.text, 0.08))
    };
    out.fg.push(Shape::rect_stroke(r, if selected { 3.0 } else { 2.0 }, border, egui::StrokeKind::Outside));
    if !selected {
        return;
    }

    // 크기 조절 손잡이 (워드처럼 모서리를 끌어서). 오른쪽 정렬이면 왼쪽 아래 모서리.
    let corner = if img.align == ImgAlign::Right { r.left_bottom() } else { r.right_bottom() };
    let hr = egui::Rect::from_center_size(corner, egui::vec2(18.0, 18.0));
    let h = ui.interact(hr, img_id.with("resize"), egui::Sense::drag());
    if h.hovered() || h.dragged() {
        ctx.set_cursor_icon(if img.align == ImgAlign::Right {
            egui::CursorIcon::ResizeNeSw
        } else {
            egui::CursorIcon::ResizeNwSe
        });
    }
    out.fg.push(Shape::rect_filled(hr.shrink(3.0), 3.0, v.accent));
    out.fg.push(Shape::rect_stroke(hr.shrink(3.0), 3.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside));
    if h.dragged()
        && let Some(p) = h.interact_pointer_pos()
    {
        let new_w = match img.align {
            ImgAlign::Left => p.x - r.left(),
            ImgAlign::Center => 2.0 * (p.x - r.center().x), // 가운데 정렬은 양쪽으로 같이
            ImgAlign::Right => r.right() - p.x,
        };
        let current = (img.size.x / img.max_w * 100.0).round() as u32;
        let pct = (new_w / img.max_w * 100.0).round().clamp(5.0, 100.0) as u32;
        if pct != current {
            out.image_action = Some(ImageAction::Width(img.line_start, Some(pct)));
        }
    }
    out.image_toolbar = Some((r, img.line_start, img.align, img.kind));
}

fn link(ui: &Ui, v: &DecoView<'_>, start: usize, end: usize, url: &str, out: &mut DecoOutput) {
    let (a, b) = (v.pos(start), v.pos(end));
    if (a.top() - b.top()).abs() > 1.0 {
        return; // 여러 줄에 걸친 링크는 첫 줄만 다루기 번거로워 건너뜀
    }
    let r = egui::Rect::from_min_max(a.min, egui::pos2(b.left(), a.bottom()));
    let ctrl = ui.input(|i| i.modifiers.command);
    if !ctrl {
        return;
    }
    if ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !v.clicked_at.is_some_and(|p| r.contains(p)) {
        return;
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    } else {
        out.follow_link = Some(url.to_owned());
    }
}
