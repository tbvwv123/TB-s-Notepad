//! 편집기 입력 도우미 연결: 목록 이어 쓰기(Enter), 들여쓰기(Tab / Shift+Tab), 토글(`>> `, Ctrl+Enter).
//! 실제 글자 바꾸기는 `crate::editing` 의 순수 함수가 합니다.

use super::*;
use crate::editing;

impl NotepadApp {
    /// 바뀐 글자와 커서(범위)를 문서와 편집기에 넣습니다.
    fn apply_edit(&mut self, ctx: &egui::Context, id: Id, text: String, sel: Range<usize>) {
        let doc = &mut self.docs[self.active];
        doc.text = text;
        doc.mark_changed();
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.start), CCursor::new(sel.end))));
        state.store(ctx, id);
        self.scroll_to_cursor = true;
        ctx.request_repaint();
    }

    /// TextEdit 보다 먼저: Enter · Tab · Shift+Tab · Ctrl+Enter 를 목록·토글에 맞게 처리합니다.
    pub(super) fn editing_keys(&mut self, ctx: &egui::Context, id: Id) {
        if !self.doc().is_markdown() || !ctx.memory(|m| m.has_focus(id)) || self.slash_is_open() {
            return;
        }
        // 한글 조합 중에 Enter 를 가로채면 조합하던 글자가 다음 줄로 가므로 건드리지 않음
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Ime(_)))) {
            return;
        }
        let Some(range) = TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()) else { return };
        let r = range.as_sorted_char_range();
        let sel = r.start.0..r.end.0;
        let pressed = |m: Modifiers, k: Key| ctx.input(|i| i.modifiers.matches_exact(m) && i.key_pressed(k));

        if pressed(Modifiers::NONE, Key::Enter)
            && sel.is_empty()
            && let Some((text, cursor)) = editing::enter_in_list(&self.doc().text, sel.start)
        {
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
            self.apply_edit(ctx, id, text, cursor..cursor);
            return;
        }
        for (mods, outdent) in [(Modifiers::NONE, false), (Modifiers::SHIFT, true)] {
            if pressed(mods, Key::Tab)
                && let Some((text, new_sel)) = editing::indent_list(&self.doc().text, sel.clone(), outdent)
            {
                ctx.input_mut(|i| i.consume_key(mods, Key::Tab));
                self.apply_edit(ctx, id, text, new_sel);
                return;
            }
        }
        // Ctrl+Enter: 토글 제목 줄에서 접기 / 펴기 (Notion 과 같음)
        if pressed(Modifiers::COMMAND, Key::Enter)
            && let Some(at) = editing::toggle_at_cursor(&self.doc().text, sel.start)
        {
            ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
            self.flip_toggle(ctx, id, at, Some(sel.start));
        }
    }

    /// TextEdit 다음: 줄 맨 앞에서 `>> ` 를 치면 그 줄을 토글로 바꿉니다 (`> ` 는 인용이라 두 개).
    pub(super) fn auto_toggle(&mut self, ctx: &egui::Context, id: Id, typed_space: bool) {
        if !typed_space || !self.is_document_mode() {
            return;
        }
        let Some(cursor) = TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0)
        else {
            return;
        };
        let text = &self.doc().text;
        let byte = markdown::char_to_byte(text, cursor);
        let line_start = text[..byte].rfind('\n').map_or(0, |i| i + 1);
        if text[line_start..byte].trim_start() == ">> " {
            let (text, c) = editing::make_toggle(text, cursor);
            self.apply_edit(ctx, id, text, c..c);
        }
    }

    /// `<details>` 줄(바이트 위치)의 토글을 접거나 폅니다. `keep_cursor` 가 있으면 그 자리(글자)로 커서를 되돌림
    /// (▶ 를 누르면 TextEdit 이 커서를 그 줄로 옮기므로).
    pub(super) fn flip_toggle(&mut self, ctx: &egui::Context, id: Id, details_line: usize, keep_cursor: Option<usize>) {
        let doc = &mut self.docs[self.active];
        let line_end = doc.text[details_line..].find('\n').map_or(doc.text.len(), |i| details_line + i);
        let before = doc.text.len();
        let Some(open) = editing::toggle_open_line(&doc.text[details_line..line_end]) else { return };
        if !editing::set_toggle_open(&mut doc.text, details_line, !open) {
            return;
        }
        doc.mark_changed();
        if let Some(c) = keep_cursor {
            // " open" 다섯 글자가 생기거나 없어진 만큼 뒤쪽 커서를 옮김
            let details_char = markdown::byte_to_char(&doc.text, details_line);
            let delta = doc.text.len() as isize - before as isize;
            let c = if c > details_char { (c as isize + delta).max(0) as usize } else { c };
            let mut state = TextEditState::load(ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            state.store(ctx, id);
        }
    }

    /// 토글 제목 줄에서 커서가 숨긴 `<summary>` 태그 안으로 들어가지 않게 제목 안으로 당깁니다.
    pub(super) fn clamp_toggle_cursor(&mut self, ctx: &egui::Context, id: Id) {
        if !self.is_document_mode() {
            return;
        }
        let Some(mut state) = TextEditState::load(ctx, id) else { return };
        let Some(range) = state.cursor.char_range().filter(|r| r.is_empty()) else { return };
        let text = &self.doc().text;
        let cursor = range.primary.index.0;
        if editing::toggle_at_cursor(text, cursor).is_none() {
            return;
        }
        let byte = markdown::char_to_byte(text, cursor);
        let line_start = text[..byte].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[byte..].find('\n').map_or(text.len(), |i| byte + i);
        let Some(title) = editing::summary_line(&text[line_start..line_end]) else { return };
        let (lo, hi) = (line_start + title.start, line_start + title.end);
        let clamped = byte.clamp(lo, hi);
        if clamped != byte {
            let c = markdown::byte_to_char(text, clamped);
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            state.store(ctx, id);
            ctx.request_repaint();
        }
    }
}
