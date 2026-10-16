//! 키보드 단축키, 끌어다 놓은 파일, 창 닫기 요청.

use super::*;

impl NotepadApp {
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
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
        if sc(cmd_shift, Key::K) {
            self.open_link_picker();
        }
        if sc(cmd_shift, Key::L) {
            self.copy_line_link(ctx);
        }
        if sc(cmd_shift, Key::H) {
            self.pending_action = Some(MdAction::Highlight);
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

    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        // 그림·소리·영상은 문서에 넣고, 메모로 열 수 없는 파일(pdf, zip …)은 탐색기 링크로 넣습니다.
        let mut insert = Vec::new();
        for path in dropped {
            if path.is_dir() {
                // 폴더를 끌어다 놓으면 메모 폴더로 추가 (Shift 를 누르고 놓으면 폴더 링크)
                if ctx.input(|i| i.modifiers.shift) {
                    insert.push(path);
                } else if !self.settings.roots.contains(&path) {
                    self.settings.roots.push(path);
                    self.sidebar.invalidate();
                }
                continue;
            }
            if is_embeddable(&path) || !has_extension(&path, TEXT_EXTENSIONS) {
                insert.push(path);
            } else {
                self.open_path(&path);
            }
        }
        if !insert.is_empty() {
            self.insert_paths(ctx, &insert);
        }
    }

    pub(super) fn handle_close_request(&mut self, ctx: &egui::Context) {
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
}
