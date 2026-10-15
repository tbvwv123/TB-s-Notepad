//! 깔끔한 화면을 위한 작은 UI 부품: 직접 그리는 아이콘, 납작한 아이콘 버튼, 세그먼트 컨트롤, 스위치.
//!
//! 아이콘을 글꼴(이모지)에 의존하지 않고 선으로 그려서 어느 PC에서나 같은 모양으로 또렷하게 보입니다.

use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};

use crate::settings::mix;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Plus,
    Close,
    Search,
    Settings,
    Sun,
    Moon,
    Image,
    Link,
    ChevronUp,
    ChevronDown,
    Bullets,
    Numbers,
    Task,
    Quote,
    Table,
    Rule,
    Replace,
    Folder,
    ChevronRight,
    Sidebar,
    More,
    Source,
    AlignLeft,
    AlignCenter,
    AlignRight,
    Trash,
}

/// 화면 색 (현재 Visuals에서 읽음)
#[derive(Clone, Copy)]
pub struct Colors {
    pub panel: Color32,
    pub editor: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
}

pub fn colors(ui: &Ui) -> Colors {
    let v = ui.visuals();
    let text = v.override_text_color.unwrap_or(v.text_color());
    Colors {
        panel: v.panel_fill,
        editor: v.extreme_bg_color,
        text,
        muted: mix(text, v.panel_fill, 0.45),
        accent: v.text_cursor.stroke.color,
    }
}

pub fn paint_icon(p: &Painter, rect: Rect, icon: Icon, color: Color32, bg: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.5; // 반지름 단위
    let st = Stroke::new(1.5, color);
    let line = |a: Pos2, b: Pos2| p.line_segment([a, b], st);
    let o = |x: f32, y: f32| pos2(c.x + x * s, c.y + y * s);

    match icon {
        Icon::Plus => {
            line(o(-0.7, 0.0), o(0.7, 0.0));
            line(o(0.0, -0.7), o(0.0, 0.7));
        }
        Icon::Close => {
            line(o(-0.5, -0.5), o(0.5, 0.5));
            line(o(-0.5, 0.5), o(0.5, -0.5));
        }
        Icon::Search => {
            let cc = o(-0.18, -0.18);
            p.circle_stroke(cc, 0.52 * s, st);
            line(cc + vec2(0.37 * s, 0.37 * s), o(0.8, 0.8));
        }
        Icon::Settings => {
            let n = 48;
            let pts: Vec<Pos2> = (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    // 톱니 8개
                    let tooth = ((i as f32 / n as f32 * 8.0).fract() < 0.5) as i32 as f32;
                    let r = s * (0.62 + 0.2 * tooth);
                    pos2(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            p.add(Shape::closed_line(pts, st));
            p.circle_stroke(c, 0.26 * s, st);
        }
        Icon::Sun => {
            p.circle_filled(c, 0.32 * s, color);
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                let d = vec2(a.cos(), a.sin());
                line(c + d * 0.55 * s, c + d * 0.82 * s);
            }
        }
        Icon::Moon => {
            p.circle_filled(c, 0.66 * s, color);
            p.circle_filled(o(0.36, -0.3), 0.56 * s, bg);
        }
        Icon::Image => {
            let r = Rect::from_center_size(c, vec2(1.6 * s, 1.3 * s));
            p.rect_stroke(r, 2.0, st, egui::StrokeKind::Middle);
            p.add(Shape::line(
                vec![
                    pos2(r.left() + 0.15 * s, r.bottom() - 0.2 * s),
                    pos2(c.x - 0.25 * s, c.y),
                    pos2(c.x + 0.15 * s, c.y + 0.35 * s),
                    pos2(c.x + 0.4 * s, c.y + 0.12 * s),
                    pos2(r.right() - 0.15 * s, r.bottom() - 0.2 * s),
                ],
                st,
            ));
            p.circle_filled(pos2(r.right() - 0.4 * s, r.top() + 0.4 * s), 0.13 * s, color);
        }
        Icon::Link => {
            let r1 = Rect::from_center_size(o(-0.4, 0.0), vec2(1.0, 0.75) * s);
            let r2 = Rect::from_center_size(o(0.4, 0.0), vec2(1.0, 0.75) * s);
            p.rect_stroke(r1, 0.37 * s, st, egui::StrokeKind::Middle);
            p.rect_stroke(r2, 0.37 * s, st, egui::StrokeKind::Middle);
        }
        Icon::ChevronUp => {
            p.add(Shape::line(vec![o(-0.5, 0.25), o(0.0, -0.25), o(0.5, 0.25)], st));
        }
        Icon::ChevronDown => {
            p.add(Shape::line(vec![o(-0.5, -0.25), o(0.0, 0.25), o(0.5, -0.25)], st));
        }
        Icon::Bullets => {
            for y in [-0.5, 0.0, 0.5] {
                p.circle_filled(o(-0.6, y), 0.1 * s, color);
                line(o(-0.3, y), o(0.75, y));
            }
        }
        Icon::Numbers => {
            // 1 2 3 숫자 대신 왼쪽에 작은 숫자 모양 + 줄
            let font = FontId::monospace(0.95 * s);
            for (i, y) in [-0.62, 0.0, 0.62].into_iter().enumerate() {
                p.text(o(-0.62, y), egui::Align2::CENTER_CENTER, (i + 1).to_string(), font.clone(), color);
                line(o(-0.2, y), o(0.85, y));
            }
        }
        Icon::Task => {
            let r = Rect::from_center_size(c, vec2(1.45 * s, 1.45 * s));
            p.rect_stroke(r, 3.0, st, egui::StrokeKind::Middle);
            p.add(Shape::line(vec![o(-0.38, 0.0), o(-0.08, 0.3), o(0.42, -0.3)], Stroke::new(1.8, color)));
        }
        Icon::Quote => {
            p.rect_filled(Rect::from_min_max(o(-0.7, -0.6), o(-0.5, 0.6)), 1.0, color);
            line(o(-0.2, -0.3), o(0.75, -0.3));
            line(o(-0.2, 0.25), o(0.5, 0.25));
        }
        Icon::Table => {
            let r = Rect::from_center_size(c, vec2(1.6 * s, 1.2 * s));
            p.rect_stroke(r, 2.0, st, egui::StrokeKind::Middle);
            line(pos2(r.left(), c.y - 0.15 * s), pos2(r.right(), c.y - 0.15 * s));
            line(pos2(r.left(), c.y + 0.25 * s), pos2(r.right(), c.y + 0.25 * s));
            line(pos2(c.x, r.top()), pos2(c.x, r.bottom()));
        }
        Icon::Rule => {
            line(o(-0.8, 0.0), o(0.8, 0.0));
            for x in [-0.8, 0.8] {
                p.circle_filled(o(x, 0.0), 0.08 * s, color);
            }
        }
        Icon::Folder => {
            let pts = vec![
                o(-0.8, -0.55),
                o(-0.25, -0.55),
                o(-0.05, -0.35),
                o(0.8, -0.35),
                o(0.8, 0.6),
                o(-0.8, 0.6),
            ];
            p.add(Shape::closed_line(pts, st));
        }
        Icon::ChevronRight => {
            p.add(Shape::line(vec![o(-0.25, -0.5), o(0.25, 0.0), o(-0.25, 0.5)], st));
        }
        Icon::Sidebar => {
            let r = Rect::from_center_size(c, vec2(1.7 * s, 1.4 * s));
            p.rect_stroke(r, 3.0, st, egui::StrokeKind::Middle);
            line(pos2(r.left() + 0.55 * s, r.top()), pos2(r.left() + 0.55 * s, r.bottom()));
        }
        Icon::More => {
            for x in [-0.55, 0.0, 0.55] {
                p.circle_filled(o(x, 0.0), 0.11 * s, color);
            }
        }
        Icon::Source => {
            p.add(Shape::line(vec![o(-0.35, -0.5), o(-0.85, 0.0), o(-0.35, 0.5)], st));
            p.add(Shape::line(vec![o(0.35, -0.5), o(0.85, 0.0), o(0.35, 0.5)], st));
            line(o(0.15, -0.6), o(-0.15, 0.6));
        }
        Icon::AlignLeft | Icon::AlignCenter | Icon::AlignRight => {
            for (i, w) in [1.5, 1.0, 1.5, 0.8].into_iter().enumerate() {
                let y = -0.6 + i as f32 * 0.4;
                let x0 = match icon {
                    Icon::AlignLeft => -0.75,
                    Icon::AlignCenter => -w / 2.0,
                    _ => 0.75 - w,
                };
                line(o(x0, y), o(x0 + w, y));
            }
        }
        Icon::Trash => {
            line(o(-0.7, -0.5), o(0.7, -0.5));
            p.add(Shape::line(vec![o(-0.25, -0.5), o(-0.2, -0.75), o(0.2, -0.75), o(0.25, -0.5)], st));
            p.add(Shape::closed_line(vec![o(-0.5, -0.5), o(-0.4, 0.75), o(0.4, 0.75), o(0.5, -0.5)], st));
            line(o(0.0, -0.2), o(0.0, 0.45));
        }
        Icon::Replace => {
            p.add(Shape::line(vec![o(-0.6, -0.1), o(-0.6, -0.45), o(0.55, -0.45)], st));
            p.add(Shape::line(vec![o(0.3, -0.7), o(0.55, -0.45), o(0.3, -0.2)], st));
            p.add(Shape::line(vec![o(0.6, 0.1), o(0.6, 0.45), o(-0.55, 0.45)], st));
            p.add(Shape::line(vec![o(-0.3, 0.2), o(-0.55, 0.45), o(-0.3, 0.7)], st));
        }
    }
}

/// 테두리 없는 버튼의 배경/글자 색 (선택됨, 누름, 올림 상태)
fn flat_colors(ui: &Ui, resp: &Response, selected: bool, base: Color32) -> (Color32, Color32) {
    let k = colors(ui);
    let bg = if selected {
        mix(base, k.accent, 0.16)
    } else if resp.is_pointer_button_down_on() {
        mix(base, k.text, 0.14)
    } else if resp.hovered() {
        mix(base, k.text, 0.08)
    } else {
        Color32::TRANSPARENT
    };
    let fg = if selected {
        k.accent
    } else if resp.hovered() {
        k.text
    } else {
        k.muted
    };
    (bg, fg)
}

/// 아이콘만 있는 납작한 버튼
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str, selected: bool, base: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 28.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let (bg, fg) = flat_colors(ui, &resp, selected, base);
        let fill_bg = if bg == Color32::TRANSPARENT { base } else { bg };
        ui.painter().rect_filled(rect.shrink(1.0), 7.0, bg);
        paint_icon(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(17.0)), icon, fg, fill_bg);
    }
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// 글자로 된 납작한 버튼 (B, I, H1 …)
pub fn text_button(ui: &mut Ui, text: egui::RichText, tooltip: &str, selected: bool, base: Color32) -> Response {
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let w = (galley.size().x + 14.0).max(30.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let (bg, fg) = flat_colors(ui, &resp, selected, base);
        ui.painter().rect_filled(rect.shrink(1.0), 7.0, bg);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    }
    resp.on_hover_text(tooltip)
}

/// iOS 스타일 켜기/끄기 스위치
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let k = colors(ui);
    let size = vec2(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let off_color = mix(k.panel, k.text, 0.22);
    let track = mix(off_color, k.accent, t);
    ui.painter().rect_filled(rect, CornerRadius::same(10), track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    ui.painter().circle_filled(pos2(x, rect.center().y), 7.5, Color32::WHITE);
    resp
}

/// "설명 ……… [스위치]" 한 줄
pub fn switch_row(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(label);
            if !hint.is_empty() {
                ui.label(egui::RichText::new(hint).small().weak());
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = switch(ui, on).changed();
        });
    });
    ui.add_space(6.0);
    changed
}

/// 설정 창 등의 섹션 제목
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).size(13.0).strong().color(colors(ui).muted));
    ui.add_space(2.0);
}
