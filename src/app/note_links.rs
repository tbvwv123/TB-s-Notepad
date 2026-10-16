//! 링크 따라가기와 만들기: 다른 메모 · 폴더 · 줄 · 제목으로 가는 링크, 이 PC의 파일 링크(탐색기),
//! "이 줄 링크 복사", 사이드바 "링크 복사", 메모 링크 고르기 창.

use super::*;
use crate::links::{self, Anchor};

/// 메모 링크 고르기 창 (Ctrl+Shift+K)
pub(super) struct LinkPicker {
    query: String,
    /// (경로, 폴더인지, 보여 줄 이름 `메모 폴더/하위/이름`)
    items: Vec<(PathBuf, bool, String)>,
    focus: bool,
}

impl NotepadApp {
    /// Ctrl+클릭한 링크 (웹 주소가 아닌 것)
    pub(super) fn follow_link(&mut self, ctx: &egui::Context, url: &str) {
        let base = self.doc().dir();
        let Some(target) = links::parse(url, base.as_deref()) else {
            self.set_status(ctx, tf!("열 수 없는 링크입니다: {}", url));
            return;
        };
        let Some(path) = target.path else {
            // 지금 메모 안
            if let Some(a) = &target.anchor {
                self.jump_to_anchor(ctx, a);
            }
            return;
        };
        if !path.exists() {
            self.set_status(ctx, tf!("찾을 수 없습니다: {}", path.display()));
            return;
        }
        if path.is_dir() {
            if self.in_roots(&path) {
                // 메모 폴더 안의 폴더: 사이드바에서 펼쳐 보여 줌
                let roots = self.settings.roots.clone();
                self.settings.sidebar_visible = true;
                self.sidebar.focus_folder(&path, &roots);
                self.set_status(ctx, tf!("사이드바에서 보여 주는 중: {}", path.display()));
            } else {
                self.reveal_in_explorer(ctx, &path);
            }
            return;
        }
        if sidebar::is_note(&path) {
            self.open_path(&path);
            let roots = self.settings.roots.clone();
            self.sidebar.reveal(&path, &roots);
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if let Some(a) = &target.anchor
                && self.doc().path.as_deref().is_some_and(|p| same_file(p, &canonical))
            {
                self.jump_to_anchor(ctx, a);
            }
            return;
        }
        self.reveal_in_explorer(ctx, &path);
    }

    /// 폴더는 탐색기로 열고, 파일은 탐색기에서 그 파일을 선택해 보여 줍니다.
    fn reveal_in_explorer(&mut self, ctx: &egui::Context, path: &Path) {
        match reveal_in_file_manager(path) {
            Ok(()) => self.set_status(ctx, tf!("탐색기에서 여는 중: {}", path.display())),
            Err(e) => self.set_status(ctx, tf!("탐색기를 열 수 없습니다: {}", e)),
        }
    }

    /// 지금 메모의 그 줄(또는 제목)을 골라 보여 줍니다.
    fn jump_to_anchor(&mut self, ctx: &egui::Context, anchor: &Anchor) {
        match links::anchor_range(&self.doc().text, anchor) {
            Some(r) => {
                self.pending_select = Some(r);
                self.focus_editor = true;
                self.selected_image = None;
            }
            None => {
                let what = match anchor {
                    Anchor::Line(n) => tf!("{}번째 줄", n),
                    Anchor::Heading(h) => tf!("'{}' 제목", h),
                };
                self.set_status(ctx, tf!("이 메모에 {}이 없습니다", what));
            }
        }
    }

    /// 커서가 있는 줄로 가는 링크를 클립보드에 복사합니다. 제목 줄이면 제목으로 가는 링크.
    pub(super) fn copy_line_link(&mut self, ctx: &egui::Context) {
        let line_no = self.cursor.line.max(1);
        self.copy_link_to_line(ctx, line_no, None);
    }

    /// `line_no` 번째 줄로 가는 하이퍼링크를 복사합니다. 제목 줄이면 제목으로 가는 링크.
    /// `label` 이 없으면 "메모 제목 n번째 줄" (제목 줄이면 "메모 제목 › 제목")
    pub(super) fn copy_link_to_line(&mut self, ctx: &egui::Context, line_no: usize, label: Option<String>) {
        let Some(path) = self.doc().path.clone() else {
            self.set_status(ctx, t!("먼저 메모를 저장해야 링크를 만들 수 있습니다"));
            return;
        };
        let line = self.doc().text.split('\n').nth(line_no.saturating_sub(1)).unwrap_or("").to_owned();
        let title = doc_display_title(self.doc());
        let (default_label, anchor) = match links::heading_text(&line).filter(|h| !links::slug(h).is_empty()) {
            Some(h) => (format!("{title} › {}", h.replace(['[', ']'], "")), links::slug(h)),
            None => (tf!("{} {}번째 줄", title, line_no), format!("L{line_no}")),
        };
        let label = label.unwrap_or(default_label);
        ctx.copy_text(format!("[{label}]({})", links::link_dest(&path, Some(&anchor))));
        self.set_status(ctx, t!("하이퍼링크를 복사했습니다. 다른 메모에 붙여넣으세요 (Ctrl+V)"));
    }

    /// 사이드바의 파일·폴더로 가는 링크를 클립보드에 복사합니다.
    pub(super) fn copy_path_link(&mut self, ctx: &egui::Context, path: &Path) {
        let is_dir = path.is_dir();
        let os = if is_dir { path.file_name() } else { path.file_stem() };
        let name = os.map(|s| s.to_string_lossy().replace(['[', ']'], "")).unwrap_or_default();
        ctx.copy_text(format!("[{name}]({})", links::link_dest(path, None)));
        self.set_status(ctx, t!("하이퍼링크를 복사했습니다. 메모에 붙여넣으세요 (Ctrl+V)"));
    }

    /// 붙여넣는 글 속 메모 링크(절대 경로)를 지금 메모 기준 상대 경로로 바꿉니다.
    /// TextEdit 이 붙여넣기 전에 불러야 합니다.
    pub(super) fn relativize_paste(&self, ctx: &egui::Context) {
        let Some(dir) = self.doc().dir() else { return };
        if !self.doc().is_markdown() {
            return;
        }
        let roots = &self.settings.roots;
        ctx.input_mut(|i| {
            for ev in &mut i.events {
                if let egui::Event::Paste(text) = ev
                    && let Some(new) = links::relativize_links(text, &dir, roots)
                {
                    *text = new;
                }
            }
        });
    }

    pub(super) fn open_link_picker(&mut self) {
        if !self.doc().is_markdown() {
            return;
        }
        let current = self.doc().path.clone();
        let mut items = Vec::new();
        for root in &self.settings.roots {
            let root_name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            items.push((root.clone(), true, root_name.clone()));
            sidebar::walk_tree(root, &self.settings.image_folder, 0, &mut |p, is_dir| {
                if items.len() >= 5000 || current.as_deref() == Some(p) {
                    return;
                }
                let rel = p.strip_prefix(root).unwrap_or(p);
                let rel = if is_dir { rel.to_path_buf() } else { rel.with_extension("") };
                let label = format!("{root_name}/{}", rel.to_string_lossy().replace('\\', "/"));
                items.push((p.to_path_buf(), is_dir, label));
            });
        }
        self.link_picker = Some(LinkPicker { query: String::new(), items, focus: true });
    }

    /// 고른 메모·폴더로 가는 링크를 커서 자리에 넣습니다 (가능하면 상대 경로).
    pub(super) fn insert_note_link(&mut self, path: &Path, is_dir: bool) {
        let os = if is_dir { path.file_name() } else { path.file_stem() };
        let name = os.map(|s| s.to_string_lossy().replace(['[', ']'], "")).unwrap_or_default();
        let dest =
            self.doc().dir().and_then(|dir| links::relative_path(&dir, path)).unwrap_or_else(|| path.to_path_buf());
        self.pending_action = Some(MdAction::Insert(format!("[{name}]({})", links::link_dest(&dest, None))));
        self.focus_editor = true;
    }

    pub(super) fn link_picker_ui(&mut self, ctx: &egui::Context) {
        let Some(picker) = &mut self.link_picker else { return };
        let mut chosen = None;
        let mut close = false;
        let modal = egui::Modal::new(Id::new("link_picker")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new(t!("메모 링크 넣기")).size(17.0).strong());
            ui.add_space(2.0);
            ui.label(
                RichText::new(t!("다른 메모나 폴더로 가는 링크를 넣습니다. 본문에서 Ctrl+클릭하면 그곳으로 갑니다."))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            let resp = ui.add(
                TextEdit::singleline(&mut picker.query)
                    .hint_text(t!("메모 · 폴더 이름으로 찾기"))
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(8, 6)),
            );
            if picker.focus {
                resp.request_focus();
                picker.focus = false;
            }
            let q = picker.query.trim().to_lowercase();
            let matches: Vec<&(PathBuf, bool, String)> =
                picker.items.iter().filter(|(_, _, label)| q.is_empty() || label.to_lowercase().contains(&q)).collect();
            if ui.input(|i| i.key_pressed(Key::Enter))
                && let Some((p, d, _)) = matches.first()
            {
                chosen = Some((p.clone(), *d));
            }
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                if matches.is_empty() {
                    ui.label(RichText::new(t!("맞는 메모가 없습니다")).weak());
                }
                let k = ui_kit::colors(ui);
                for (p, is_dir, label) in matches.iter().take(300) {
                    let (rect, row) =
                        ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
                    if row.hovered() {
                        ui.painter().rect_filled(rect, 6.0, mix(k.panel, k.text, 0.07));
                    }
                    let icon_rect = egui::Rect::from_center_size(
                        rect.left_center() + egui::vec2(14.0, 0.0),
                        egui::vec2(14.0, 14.0),
                    );
                    if *is_dir {
                        ui_kit::paint_icon(ui.painter(), icon_rect, Icon::Folder, k.muted, k.panel);
                    } else {
                        ui.painter().circle_stroke(icon_rect.center(), 4.5, Stroke::new(1.3, k.muted));
                    }
                    ui.painter().text(
                        rect.left_center() + egui::vec2(30.0, 0.0),
                        Align2::LEFT_CENTER,
                        label,
                        FontId::proportional(13.5),
                        k.text,
                    );
                    if row.clicked() {
                        chosen = Some((p.clone(), *is_dir));
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(t!(
                    "특정 줄로 가는 링크는 그 메모에서 ⋯ › 편집 › 이 줄 링크 복사 (Ctrl+Shift+L) 후 붙여넣기"
                ))
                .small()
                .weak(),
            );
            ui.add_space(6.0);
            let row = egui::vec2(ui.available_width(), 28.0);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.button(t!("닫기")).clicked() {
                    close = true;
                }
            });
        });
        if modal.should_close() {
            close = true;
        }
        if let Some((p, is_dir)) = chosen {
            self.link_picker = None;
            self.insert_note_link(&p, is_dir);
        } else if close {
            self.link_picker = None;
            self.focus_editor = true;
        }
    }
}
