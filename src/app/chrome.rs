//! 편집기 둘레의 화면: ⋯ 메뉴, 사이드바 머리, 상단 바, 서식 도구, 상태 표시줄.

use super::*;

impl NotepadApp {
    /// ⋯ 버튼 메뉴
    pub(super) fn main_menu(&mut self, ui: &mut Ui) {
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
        ui.add_enabled_ui(is_md, |ui| {
            ui.menu_button("서식", |ui| {
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
            })
        });
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

    pub(super) fn open_settings(&mut self, page: SettingsPage) {
        self.show_settings = true;
        self.settings_page = page;
    }

    pub(super) fn sidebar_ui(&mut self, ui: &mut Ui) {
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
    pub(super) fn top_bar(&mut self, ui: &mut Ui) {
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
                    let comps: Vec<String> =
                        rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
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
                    if ui_kit::icon_button(ui, Icon::Source, "마크다운 원본 보기 (Ctrl+E)", source, base).clicked()
                    {
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
    pub(super) fn toolbar(&mut self, ui: &mut Ui) {
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
            if ui_kit::icon_button(
                ui,
                Icon::Image,
                "그림 넣기\n복사한 그림은 Ctrl+V, 파일은 창에 끌어다 놓아도 됩니다",
                false,
                base,
            )
            .clicked()
            {
                self.insert_image_dialog();
            }
        });
        if action.is_some() {
            self.pending_action = action;
        }
    }

    /// 아래쪽 흐린 글씨 한 줄
    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
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
            ui.label(
                RichText::new(format!("{}자 · {}단어", fmt_num(chars), fmt_num(markdown::word_count(&doc.text))))
                    .small()
                    .weak(),
            );
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
                if ui.add(flat(format!("{pct}%"))).on_hover_text("글자 크기 (Ctrl+휠) · 누르면 기본 크기").clicked()
                {
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
}

/// 메뉴 항목 (오른쪽에 단축키 표시). 누르면 메뉴를 닫고 `true`.
pub(super) fn item(ui: &mut Ui, label: &str, shortcut: &str) -> bool {
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

pub(super) fn fmt_num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
