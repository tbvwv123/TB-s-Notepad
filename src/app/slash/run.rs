//! 고른 명령 실행: 친 `/검색어` 를 지우고 서식을 넣거나 창을 엽니다.

use super::super::*;
use super::commands::Cmd;
use super::{MenuKind, SlashMenu};

impl NotepadApp {
    /// 친 글자(`:검색어`)를 `text` 로 바꾸고 커서를 그 뒤로
    pub(super) fn replace_typed(&mut self, ctx: &egui::Context, id: Id, typed: Range<usize>, text: &str) {
        let doc = &mut self.docs[self.active];
        let (a, b) = (markdown::char_to_byte(&doc.text, typed.start), markdown::char_to_byte(&doc.text, typed.end));
        doc.text.replace_range(a..b, text);
        doc.mark_changed();
        set_caret(ctx, id, typed.start + text.chars().count());
        self.focus_editor = true;
        ctx.request_repaint();
    }

    /// 목록을 열려고 친 글자(`/검색어`)를 지우고 커서를 그 자리에
    pub(super) fn remove_typed(&mut self, ctx: &egui::Context, id: Id, typed: Range<usize>) {
        self.replace_typed(ctx, id, typed, "");
    }

    /// `/검색어` 글자를 지우고 명령을 실행합니다.
    pub(super) fn run_slash(&mut self, ctx: &egui::Context, id: Id, typed: Range<usize>, cmd: Cmd) {
        self.remove_typed(ctx, id, typed.clone());
        let md = |a: MdAction| Some(a);
        let action = match cmd {
            Cmd::Heading(l) => md(MdAction::Heading(l)),
            Cmd::Bullet => md(MdAction::Bullet),
            Cmd::Numbered => md(MdAction::Numbered),
            Cmd::Task => md(MdAction::Task),
            Cmd::Quote => md(MdAction::Quote),
            Cmd::Toggle => md(MdAction::Toggle),
            Cmd::Subtext => md(MdAction::Subtext),
            Cmd::Highlight => md(MdAction::Highlight),
            Cmd::CodeBlock => md(MdAction::CodeBlock),
            Cmd::Table => md(MdAction::Table),
            Cmd::Rule => md(MdAction::Rule),
            Cmd::WebLink => md(MdAction::Link),
            Cmd::DateTime => md(MdAction::Insert(chrono::Local::now().format("%Y-%m-%d %H:%M").to_string())),
            Cmd::Date => md(MdAction::Insert(chrono::Local::now().format("%Y-%m-%d").to_string())),
            Cmd::NoteLink => {
                self.open_link_picker();
                None
            }
            Cmd::CopyLineLink => {
                self.copy_line_link(ctx);
                None
            }
            Cmd::FileLink => {
                self.insert_file_link_dialog(false);
                None
            }
            Cmd::FolderLink => {
                self.insert_file_link_dialog(true);
                None
            }
            Cmd::Media => {
                self.insert_image_dialog();
                None
            }
            Cmd::PasteImage => {
                self.paste_clipboard_image(ctx);
                None
            }
            Cmd::Emoji => {
                // `:` 를 넣고 이모지 목록을 엶 (이어서 치면 걸러짐)
                let at = typed.start;
                let doc = &mut self.docs[self.active];
                doc.text.insert(markdown::char_to_byte(&doc.text, at), ':');
                doc.mark_changed();
                set_caret(ctx, id, at + 1);
                let emoji = self.emoji_list();
                self.slash = Some(SlashMenu::new(self.doc().id, at, MenuKind::Emoji, true, emoji));
                None
            }
        };
        // 토글 제목 줄: 줄 맨 앞에 기호를 붙이면 <summary> 앞에 붙어 토글이 깨지므로
        // 제목은 제목 글자 앞에 `## ` 를 넣고, 다른 줄 서식은 하지 않음
        let in_title = crate::editing::toggle_at_cursor(&self.doc().text, typed.start).is_some();
        let action = match action {
            Some(MdAction::Heading(l)) if in_title => Some(MdAction::Insert(format!("{} ", "#".repeat(l)))),
            Some(
                MdAction::Bullet
                | MdAction::Numbered
                | MdAction::Task
                | MdAction::Quote
                | MdAction::Subtext
                | MdAction::Toggle
                | MdAction::CodeBlock
                | MdAction::Table
                | MdAction::Rule,
            ) if in_title => None,
            a => a,
        };
        // 바로 적용: 서식 도구와 달리 줄을 선택해 두지 않고, 커서를 기호 바로 뒤에 둬서 이어 쓰게 합니다.
        if let Some(action) = action {
            let doc = &mut self.docs[self.active];
            let at = typed.start;
            let before = doc.text.chars().count();
            let sel = markdown::apply_action(&mut doc.text, at..at, &action);
            // 줄 앞에 붙은(또는 빠진) 기호만큼 커서를 옮김
            let shifted = (at + doc.text.chars().count()).saturating_sub(before);
            doc.mark_changed();
            let line_prefix = matches!(
                action,
                MdAction::Heading(_)
                    | MdAction::Bullet
                    | MdAction::Numbered
                    | MdAction::Task
                    | MdAction::Quote
                    | MdAction::Subtext
            );
            set_selection(ctx, id, if line_prefix { shifted..shifted } else { sel });
        }
        ctx.request_repaint();
    }
}
