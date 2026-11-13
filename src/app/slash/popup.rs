//! 목록 창 그리기 (명령 · 이모지 공통).

use super::super::*;
use super::commands::{Badge, COMMANDS};
use super::{Item, MenuKind, SlashMenu};

/// 목록 창. 누른 줄과 창 자리를 돌려줍니다.
pub(super) fn popup_ui(
    ctx: &egui::Context,
    menu: &mut SlashMenu,
    caret: egui::Rect,
    items: &[Item],
    query: &str,
) -> (Option<Item>, egui::Rect) {
    const ROW_H: f32 = 34.0;
    const WIDTH: f32 = 300.0;
    let list_h = (items.len().max(1) as f32 * ROW_H).min(ROW_H * 8.0);
    let total_h = list_h + 44.0;
    let screen = ctx.content_rect();
    let below = caret.bottom() + 6.0;
    let y = if below + total_h > screen.bottom() - 8.0 { (caret.top() - total_h - 6.0).max(8.0) } else { below };
    let x = caret.left().min(screen.right() - WIDTH - 24.0).max(8.0);
    let mut clicked = None;
    let area = egui::Area::new(Id::new("slash_menu")).order(egui::Order::Foreground).fixed_pos(egui::pos2(x, y)).show(
        ctx,
        |ui| {
            Frame::popup(ui.style()).inner_margin(Margin::same(6)).corner_radius(10).show(ui, |ui| {
                ui.set_width(WIDTH);
                let k = ui_kit::colors(ui);
                if items.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(t!("  맞는 명령이 없습니다")).color(k.muted));
                    ui.add_space(6.0);
                }
                ScrollArea::vertical().max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (row, &item) in items.iter().enumerate() {
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());
                        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            menu.selected = row;
                        }
                        let selected = menu.selected == row;
                        if selected && menu.scroll {
                            resp.scroll_to_me(None);
                            menu.scroll = false;
                        }
                        let p = ui.painter();
                        if selected {
                            p.rect_filled(rect, 7.0, mix(k.panel, k.text, 0.08));
                        }
                        let badge = egui::Rect::from_center_size(
                            rect.left_center() + egui::vec2(19.0, 0.0),
                            egui::vec2(26.0, 26.0),
                        );
                        let badge_bg = mix(k.panel, k.text, if selected { 0.12 } else { 0.06 });
                        let text_badge = |t: &str| {
                            p.rect_filled(badge, 6.0, badge_bg);
                            p.text(badge.center(), Align2::CENTER_CENTER, t, FontId::monospace(11.5), k.text);
                        };
                        let (label, hint): (String, &str) = match item {
                            Item::Cmd(i) => {
                                let cmd = &COMMANDS[i];
                                match cmd.badge {
                                    Badge::Icon(icon) => {
                                        p.rect_filled(badge, 6.0, badge_bg);
                                        ui_kit::paint_icon(p, badge.shrink(6.0), icon, k.text, k.panel);
                                    }
                                    Badge::Text(t) => text_badge(t),
                                }
                                (crate::i18n::tr(cmd.label).to_owned(), cmd.hint)
                            }
                            Item::Emoji(j) => {
                                let e = &menu.emoji[j];
                                let uri = crate::markdown::file_uri(&e.path);
                                if let Ok(Some(shape)) = ui_kit::image_shape(ctx, &uri, badge.shrink(1.0), true) {
                                    p.add(shape);
                                }
                                (format!(":{}:", e.name), "")
                            }
                            Item::Builtin(i) => {
                                let uri = crate::twemoji::uri(i);
                                if let Ok(Some(shape)) = ui_kit::image_shape(ctx, &uri, badge.shrink(2.0), true) {
                                    p.add(shape);
                                }
                                (format!(":{}:", crate::twemoji::TWEMOJI[i].0), "")
                            }
                            Item::AddEmoji => {
                                text_badge("+");
                                let name = crate::emoji::clean_name(query);
                                let label = if name.is_empty() {
                                    t!("그림으로 새 이모지 추가…").to_owned()
                                } else {
                                    tf!(":{}: 새 이모지로 추가…", name)
                                };
                                (label, "")
                            }
                        };
                        p.text(
                            rect.left_center() + egui::vec2(42.0, 0.0),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(14.0),
                            k.text,
                        );
                        if !hint.is_empty() {
                            p.text(
                                rect.right_center() - egui::vec2(10.0, 0.0),
                                Align2::RIGHT_CENTER,
                                hint,
                                FontId::proportional(11.5),
                                k.muted,
                            );
                        }
                        if resp.clicked() {
                            clicked = Some(item);
                        }
                    }
                });
                ui.add_space(4.0);
                let help = match menu.kind {
                    MenuKind::Commands => t!("  ↑↓ 고르기 · Enter 실행 · Space / Esc 닫기"),
                    MenuKind::Emoji => t!("  ↑↓ 고르기 · Enter 넣기 · Space / Esc 닫기"),
                };
                ui.label(RichText::new(help).size(11.0).color(k.muted));
            });
        },
    );
    (clicked, area.response.rect)
}
