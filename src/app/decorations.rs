//! 문서 모드 꾸밈 그리기: 코드 블록 배경, 인용 막대, 목록 점, 체크박스, 구분선,
//! 그림(선택 테두리·크기 손잡이), 소리·영상 카드, Ctrl+클릭 링크(웹 주소는 브라우저, 나머지는 `note_links.rs`).
//!
//! 무엇을 어디에 그릴지는 `live.rs`가 [`Deco`]로 알려 주고, 여기서는 배치된 글자 위치에 맞춰 그립니다.

use super::media_cards::{MediaAction, MediaHub, audio_card, media_path, video_card};
use super::*;
use crate::fonts::fake_bold_offsets;
use crate::media::MediaKind;

/// 꾸밈을 그릴 때 필요한 편집기 상태
pub(super) struct DecoView<'a> {
    pub galley: &'a egui::Galley,
    /// 줄(배치된 행)마다 시작 글자 위치. 글자 위치 → 화면 위치를 이진 탐색으로 찾으려고 한 번 만들어 둠
    pub row_starts: Vec<usize>,
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
    /// 굵은 글꼴이 없어 겹쳐 그려 굵게 보이게 하는지
    pub fake_bold: bool,
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
    /// 누른 토글 ▶ (`<details>` 줄의 바이트)
    pub flip_toggle: Option<usize>,
    /// Ctrl+클릭한 태그 (그 태그가 붙은 메모를 모아 봄)
    pub follow_tag: Option<String>,
}

/// 행마다 시작 글자 위치 (`Galley::pos_from_cursor` 는 매번 처음부터 세므로 큰 문서에서 느림)
pub(super) fn row_starts(galley: &egui::Galley) -> Vec<usize> {
    let mut out = Vec::with_capacity(galley.rows.len());
    let mut at = 0;
    for row in &galley.rows {
        out.push(at);
        at += row.char_count_including_newline().0;
    }
    out
}

/// 글자 위치의 커서 모양 (폭 0 인 사각형). `Galley::pos_from_cursor` 와 같은 결과를 이진 탐색으로.
pub(super) fn pos_in(galley: &egui::Galley, row_starts: &[usize], ci: usize) -> egui::Rect {
    let rows = &galley.rows;
    let mut r = row_starts.partition_point(|&s| s <= ci).saturating_sub(1);
    // 자동 줄바꿈으로 나뉜 행의 끝 = 다음 행의 시작이면 앞 행에 둠 (egui 와 같게)
    if r > 0 && row_starts[r] == ci && !rows[r - 1].ends_with_newline {
        r -= 1;
    }
    let Some(row) = rows.get(r) else { return galley.pos_from_cursor(CCursor::new(ci)) };
    let col = (ci - row_starts[r]).min(row.char_count_including_newline().0);
    let x = row.x_offset(egui::text::CharIndex(col)) + row.pos.x;
    egui::Rect::from_min_max(egui::pos2(x, row.min_y()), egui::pos2(x, row.max_y()))
}

impl DecoView<'_> {
    fn pos(&self, ci: usize) -> egui::Rect {
        pos_in(self.galley, &self.row_starts, ci).translate(self.origin)
    }
}

pub(super) fn paint_decorations(ui: &mut Ui, view: &DecoView<'_>, decos: &[Deco]) -> DecoOutput {
    let mut out = DecoOutput::default();
    for d in decos {
        match d {
            Deco::CodeBlock { start, end } => code_block(view, *start, *end, &mut out),
            Deco::Quote { start, end } => quote(view, *start, *end, &mut out),
            Deco::Bullet { at, level, .. } => bullet(view, *at, *level, &mut out),
            Deco::Task { at, checked, byte, .. } => task(view, *at, *checked, *byte, &mut out),
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
            Deco::Emoji { start, uri, size, .. } => emoji(ui, view, *start, uri, *size, &mut out),
            Deco::Twemoji { start, end, idx, size } => twemoji(ui, view, *start, *end, *idx, *size, &mut out),
            Deco::Tag { start, end, tag } => tag_chip(ui, view, *start, *end, tag, &mut out),
            Deco::Table { rows, aligns, row_height } => table(ui, view, rows, aligns, *row_height, &mut out),
            Deco::Toggle { at, open, details_byte, depth, empty } => {
                toggle(ui, view, *at, *open, *details_byte, *depth, *empty, &mut out)
            }
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

/// 표: 칸 폭은 글자에 맞추되 본문 폭을 넘으면 줄이고, 넘치는 글자는 … 로 줄입니다.
fn table(ui: &Ui, v: &DecoView<'_>, rows: &[live::TableRow], aligns: &[ImgAlign], row_h: f32, out: &mut DecoOutput) {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else { return };
    let top = v.pos(first.at).top();
    let bottom = v.pos(last.at).top() + row_h;
    if bottom < v.clip.top() || top > v.clip.bottom() {
        return;
    }
    let cols = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0).max(aligns.len());
    if cols == 0 {
        return;
    }
    let body = FontId::new(v.font_size, doc_family());
    let bold = FontId::new(v.font_size, bold_family());
    let pad = 12.0;
    let mut widths = vec![56.0f32; cols];
    for r in rows {
        let font = if r.header { &bold } else { &body };
        for (c, cell) in r.cells.iter().enumerate() {
            let w = ui.fonts_mut(|f| f.layout_no_wrap(cell.clone(), font.clone(), v.text).size().x);
            widths[c] = widths[c].max(w + pad * 2.0);
        }
    }
    let avail = (v.text_right - v.text_left).max(80.0);
    let total: f32 = widths.iter().sum();
    if total > avail {
        widths.iter_mut().for_each(|w| *w *= avail / total);
    }
    let x0 = v.text_left;
    let table_w: f32 = widths.iter().sum();
    let outer = egui::Rect::from_min_max(egui::pos2(x0, top), egui::pos2(x0 + table_w, bottom));
    let line = Stroke::new(1.0, mix(v.bg, v.text, 0.16));

    // 머리 행 배경과 줄무늬
    for (i, r) in rows.iter().enumerate() {
        let y = v.pos(r.at).top();
        let row = egui::Rect::from_x_y_ranges(outer.x_range(), y..=y + row_h);
        let radius = if i == 0 { egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 } } else { egui::CornerRadius::ZERO };
        if r.header {
            out.bg.push(Shape::rect_filled(row, radius, mix(v.bg, v.text, 0.06)));
        }
        if i > 0 {
            let s = if rows[i - 1].header { Stroke::new(1.3, mix(v.bg, v.text, 0.28)) } else { line };
            out.fg.push(Shape::line_segment([egui::pos2(outer.left(), y), egui::pos2(outer.right(), y)], s));
        }
    }
    let mut x = x0;
    for w in &widths[..cols - 1] {
        x += w;
        out.fg.push(Shape::line_segment([egui::pos2(x, top), egui::pos2(x, bottom)], line));
    }
    out.fg.push(Shape::rect_stroke(outer, 6.0, line, egui::StrokeKind::Inside));

    // 글자
    for r in rows {
        let y = v.pos(r.at).top();
        let font = if r.header { &bold } else { &body };
        let mut x = x0;
        for (c, w) in widths.iter().enumerate() {
            if let Some(cell) = r.cells.get(c).filter(|s| !s.is_empty()) {
                let mut job = egui::text::LayoutJob::simple_singleline(cell.clone(), font.clone(), v.text);
                job.wrap = egui::text::TextWrapping::truncate_at_width((w - pad * 2.0).max(8.0));
                let g = ui.fonts_mut(|f| f.layout_job(job));
                let gx = match aligns.get(c).copied().unwrap_or(ImgAlign::Left) {
                    ImgAlign::Left => x + pad,
                    ImgAlign::Center => x + (w - g.size().x) / 2.0,
                    ImgAlign::Right => x + w - pad - g.size().x,
                };
                let pos = egui::pos2(gx, y + (row_h - g.size().y) / 2.0);
                if r.header && v.fake_bold {
                    for dx in fake_bold_offsets(v.font_size, ui.ctx().pixels_per_point()) {
                        out.fg.push(Shape::galley(pos + egui::vec2(dx, 0.0), g.clone(), v.text));
                    }
                }
                out.fg.push(Shape::galley(pos, g, v.text));
            }
            x += w;
        }
    }
}

/// 토글 제목 앞의 ▶ (접힘) / ▼ (펼침). 누르면 접기 · 펴기
#[allow(clippy::too_many_arguments)]
fn toggle(
    ui: &Ui,
    v: &DecoView<'_>,
    at: usize,
    open: bool,
    details_byte: usize,
    depth: usize,
    empty: bool,
    out: &mut DecoOutput,
) {
    let a = v.pos(at);
    if !a.intersects(v.clip) {
        return;
    }
    let w = v.font_size * live::TOGGLE_INDENT_SCALE;
    let center = egui::pos2(v.text_left + (depth as f32 + 0.5) * w, a.center().y);
    let hit = egui::Rect::from_center_size(center, egui::vec2(w, a.height().max(v.font_size)));
    let hovered = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| hit.contains(p));
    if hovered {
        out.fg.push(Shape::rect_filled(hit.shrink(2.0), 5.0, mix(v.bg, v.text, 0.08)));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let s = v.font_size * 0.3;
    let pts = if open {
        vec![center + egui::vec2(-s, -s * 0.6), center + egui::vec2(s, -s * 0.6), center + egui::vec2(0.0, s * 0.8)]
    } else {
        vec![center + egui::vec2(-s * 0.6, -s), center + egui::vec2(s * 0.8, 0.0), center + egui::vec2(-s * 0.6, s)]
    };
    out.fg.push(Shape::convex_polygon(pts, v.text, Stroke::NONE));
    if empty {
        let g = ui.painter().layout_no_wrap(t!("토글").to_owned(), egui::FontId::proportional(v.font_size), v.muted);
        out.fg.push(Shape::galley(egui::pos2(a.left() + 2.0, a.center().y - g.size().y / 2.0), g, v.muted));
    }
    if v.clicked_at.is_some_and(|p| hit.contains(p)) {
        out.flip_toggle = Some(details_byte);
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

/// `#태그`: 글자 아래에 둥근 바탕, Ctrl+클릭하면 모아 보기
fn tag_chip(ui: &Ui, v: &DecoView<'_>, start: usize, end: usize, tag: &str, out: &mut DecoOutput) {
    let (a, b) = (v.pos(start), v.pos(end));
    if (a.top() - b.top()).abs() > 1.0 {
        return; // 줄이 바뀌며 나뉜 태그는 바탕 없이
    }
    let h = (a.height() * 0.78).min(v.font_size * 1.35);
    let r = egui::Rect::from_min_max(
        egui::pos2(a.left() - 1.0, a.center().y - h / 2.0),
        egui::pos2(b.left() + 1.0, a.center().y + h / 2.0),
    );
    if !r.intersects(v.clip) {
        return;
    }
    out.bg.push(Shape::rect_filled(r, h / 2.0, mix(v.bg, v.accent, 0.13)));
    if !ui.input(|i| i.modifiers.command) {
        return;
    }
    if ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if v.clicked_at.is_some_and(|p| r.contains(p)) {
        out.follow_tag = Some(tag.to_owned());
    }
}

/// 유니코드 이모지 글자 자리(투명하게 둔 글자)에 트위모지 그림
fn twemoji(ui: &Ui, v: &DecoView<'_>, start: usize, end: usize, idx: usize, size: f32, out: &mut DecoOutput) {
    let (a, b) = (v.pos(start), v.pos(end));
    // 다음 글자 앞에 비워 둔 자리 (줄 끝이면 다음 글자가 없어 이모지 오른쪽에 그대로)
    let slot = b.left() - a.left();
    let cx = if (a.top() - b.top()).abs() < 1.0 && slot > size * 0.6 {
        (a.left() + b.left()) / 2.0
    } else {
        a.left() + 1.5 + size / 2.0
    };
    let r = egui::Rect::from_center_size(egui::pos2(cx, a.center().y), egui::vec2(size, size));
    if !r.intersects(v.clip) {
        return;
    }
    let uri = crate::twemoji::uri(idx);
    if let Ok(egui::load::TexturePoll::Ready { texture }) =
        ui.ctx().try_load_texture(&uri, egui::TextureOptions::LINEAR, egui::SizeHint::default())
    {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        out.fg.push(Shape::image(texture.id, r, uv, Color32::WHITE));
    }
}

/// 글 속 이모지: 첫 글자 뒤에 비워 둔 자리에 그림을 (비율을 지켜) 그립니다.
fn emoji(ui: &Ui, v: &DecoView<'_>, start: usize, uri: &str, size: f32, out: &mut DecoOutput) {
    // 자리를 비워 둔 것은 둘째 글자 앞이므로, 그 글자의 왼쪽이 그림 자리의 오른쪽 끝
    let after = v.pos(start + 1);
    let slot = egui::Rect::from_center_size(
        egui::pos2(after.left() - size / 2.0 - 2.0, after.center().y),
        egui::vec2(size, size),
    );
    if !slot.intersects(v.clip) {
        return;
    }
    match ui.ctx().try_load_texture(uri, egui::TextureOptions::LINEAR, egui::SizeHint::default()) {
        Ok(egui::load::TexturePoll::Ready { texture }) => {
            let s = texture.size;
            let scale = if s.x > 0.0 && s.y > 0.0 { (size / s.x).min(size / s.y) } else { 1.0 };
            let r = egui::Rect::from_center_size(slot.center(), s * scale);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            out.fg.push(Shape::image(texture.id, r, uv, Color32::WHITE));
        }
        Ok(egui::load::TexturePoll::Pending { .. }) => {}
        // 그림이 없거나 못 읽으면 빈 칸 표시
        Err(_) => out.fg.push(Shape::rect_stroke(
            slot.shrink(2.0),
            3.0,
            Stroke::new(1.0, mix(v.bg, v.muted, 0.7)),
            egui::StrokeKind::Inside,
        )),
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
    if img.kind == MediaKind::Image {
        resp.context_menu(|ui| {
            if ui.button(t!("이모지로 등록…")).clicked() {
                out.image_action = Some(ImageAction::RegisterEmoji(img.line_start));
                ui.close();
            }
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 이진 탐색 위치가 egui 의 `pos_from_cursor` 와 모든 글자에서 같은지 (자동 줄바꿈 행 경계 포함)
    #[test]
    fn fast_positions_match_egui() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx().clone();
            let text = "첫 줄은 꽤 길어서 자동으로 여러 행에 걸쳐 나뉘도록 글자를 많이 씁니다 abc def ghi

둘째 줄
- 목록 항목 하나 둘 셋 넷 다섯 여섯 일곱 여덟
";
            let mut job =
                egui::text::LayoutJob::simple(text.to_owned(), egui::FontId::proportional(14.0), Color32::WHITE, 120.0);
            job.wrap.max_width = 120.0;
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            assert!(
                galley.rows.iter().any(|r| !r.ends_with_newline) && galley.rows.len() > 6,
                "줄바꿈된 행이 있어야 함"
            );
            let starts = row_starts(&galley);
            for ci in 0..=text.chars().count() {
                assert_eq!(pos_in(&galley, &starts, ci), galley.pos_from_cursor(CCursor::new(ci)), "글자 {ci}");
            }
        });
        // 테스트에서는 글꼴 텍스처를 쓰지 않으므로 버림
        out.textures_delta.clear();
    }
}
