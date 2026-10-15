//! 이전 버전 창 (⋯ › 이전 버전, 사이드바 오른쪽 클릭): 왼쪽에 기록 목록, 오른쪽에 미리보기, 되돌리기.

use super::*;
use crate::history::{self, Snapshot};

pub(super) struct HistoryView {
    path: PathBuf,
    items: Vec<Snapshot>,
    selected: usize,
    /// (고른 번호, 내용)
    preview: Option<(usize, String)>,
}

fn time_label(t: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d  %H:%M").to_string()
}

fn size_label(n: u64) -> String {
    if n < 1024 { format!("{n} B") } else { format!("{:.1} KB", n as f64 / 1024.0) }
}

impl NotepadApp {
    pub(super) fn open_history(&mut self, path: &Path) {
        self.history_view =
            Some(HistoryView { path: path.to_path_buf(), items: history::list(path), selected: 0, preview: None });
    }

    pub(super) fn history_ui(&mut self, ctx: &egui::Context) {
        let Some(view) = &mut self.history_view else { return };
        if view.preview.as_ref().is_none_or(|(i, _)| *i != view.selected)
            && let Some(item) = view.items.get(view.selected)
        {
            let text = Document::open(&item.file).map(|d| d.text).unwrap_or_default();
            view.preview = Some((view.selected, text));
        }
        let mut close = false;
        let mut restore = None;
        let modal = egui::Modal::new(Id::new("history")).show(ctx, |ui| {
            ui.set_width(720.0);
            let name = view.path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            ui.label(RichText::new(tf!("이전 버전 · {}", name)).size(17.0).strong());
            ui.label(
                RichText::new(t!(
                    "고치는 동안 5분마다, 그리고 많이 지웠을 때 저장 직전의 내용을 남겨 둡니다 (메모마다 50개까지)."
                ))
                .small()
                .weak(),
            );
            ui.add_space(8.0);
            if view.items.is_empty() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(t!("아직 이전 버전이 없습니다")).weak()));
                ui.add_space(30.0);
            } else {
                let k = ui_kit::colors(ui);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        ScrollArea::vertical().id_salt("history_list").max_height(380.0).show(ui, |ui| {
                            for (i, item) in view.items.iter().enumerate() {
                                let (rect, resp) =
                                    ui.allocate_exact_size(egui::vec2(190.0, 40.0), egui::Sense::click());
                                let fill = if i == view.selected {
                                    mix(k.panel, k.accent, 0.16)
                                } else if resp.hovered() {
                                    mix(k.panel, k.text, 0.06)
                                } else {
                                    Color32::TRANSPARENT
                                };
                                ui.painter().rect_filled(rect, 7.0, fill);
                                ui.painter().text(
                                    rect.left_top() + egui::vec2(10.0, 6.0),
                                    Align2::LEFT_TOP,
                                    time_label(item.time),
                                    FontId::proportional(13.5),
                                    k.text,
                                );
                                ui.painter().text(
                                    rect.left_top() + egui::vec2(10.0, 23.0),
                                    Align2::LEFT_TOP,
                                    size_label(item.size),
                                    FontId::proportional(11.5),
                                    k.muted,
                                );
                                if resp.clicked() {
                                    view.selected = i;
                                }
                            }
                        });
                    });
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.set_max_height(380.0);
                        let text = view.preview.as_ref().map(|(_, t)| t.as_str()).unwrap_or("");
                        Frame::NONE.fill(k.editor).corner_radius(8).inner_margin(Margin::same(10)).show(ui, |ui| {
                            ScrollArea::vertical().id_salt("history_preview").max_height(360.0).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(text).font(FontId::new(13.5, editor_family())));
                            });
                        });
                    });
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if !view.items.is_empty() && ui.button(t!("이 버전으로 되돌리기")).clicked() {
                    restore = view.preview.as_ref().map(|(_, t)| t.clone());
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(t!("닫기")).clicked() {
                        close = true;
                    }
                });
            });
        });
        if modal.should_close() {
            close = true;
        }
        if let Some(text) = restore {
            let path = view.path.clone();
            self.history_view = None;
            self.restore_version(ctx, &path, text);
        } else if close {
            self.history_view = None;
        }
    }

    /// 고른 버전으로 되돌립니다. 지금 내용도 먼저 기록해 두므로 되돌리기를 다시 되돌릴 수 있습니다.
    fn restore_version(&mut self, ctx: &egui::Context, path: &Path, text: String) {
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(path)) {
            self.save_if_needed(self.docs[i].id);
        }
        history::record_now(path);
        self.open_path(path);
        let doc = &mut self.docs[self.active];
        if doc.path.as_deref() == Some(path) {
            doc.text = text;
            doc.mark_changed();
            self.set_status(ctx, t!("이전 버전으로 되돌렸습니다. 되돌리기 전 내용도 이전 버전에 남아 있습니다"));
        }
    }
}
