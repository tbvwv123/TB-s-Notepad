//! 본문 오른쪽 클릭 메뉴: 잘라내기 · 복사 · 붙여넣기 · 모두 선택, 하이퍼링크 복사, 링크 만들기, 메모 링크 넣기.
//!
//! egui 의 TextEdit 은 오른쪽 클릭에도 커서를 옮겨 선택이 풀리므로, 선택한 글자 위를 오른쪽 클릭하면
//! 선택을 되살리고 그 범위를 기억해 두었다가 메뉴에서 씁니다.

use super::*;
use crate::links;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum EditMenu {
    Cut,
    Copy,
    Paste,
    SelectAll,
    CopyHyperlink,
    MakeLink,
    NoteLink,
}

/// 메뉴를 그리고 고른 항목을 돌려줍니다. `has_sel`: 선택한 글자가 있는지
pub(super) fn edit_menu_ui(ui: &mut Ui, has_sel: bool, is_md: bool, has_path: bool) -> Option<EditMenu> {
    let mut picked = None;
    ui.set_min_width(210.0);
    let mut item = |ui: &mut Ui, label: &str, key: &str, enabled: bool, what: EditMenu| {
        let button = egui::Button::new(label).shortcut_text(key);
        if ui.add_enabled(enabled, button).clicked() {
            picked = Some(what);
            ui.close();
        }
    };
    item(ui, t!("잘라내기"), "Ctrl+X", has_sel, EditMenu::Cut);
    item(ui, t!("복사"), "Ctrl+C", has_sel, EditMenu::Copy);
    item(ui, t!("붙여넣기"), "Ctrl+V", true, EditMenu::Paste);
    item(ui, t!("모두 선택"), "Ctrl+A", true, EditMenu::SelectAll);
    if is_md {
        ui.separator();
        let label = if has_sel {
            t!("하이퍼링크 복사 (선택한 글자로)")
        } else {
            t!("하이퍼링크 복사 (이 줄)")
        };
        item(ui, label, "Ctrl+Shift+L", has_path, EditMenu::CopyHyperlink);
        item(ui, t!("링크 만들기"), "Ctrl+K", true, EditMenu::MakeLink);
        item(ui, t!("메모 링크 넣기…"), "Ctrl+Shift+K", true, EditMenu::NoteLink);
    }
    picked
}

/// 글자 위치(커서 단위)가 몇 번째 줄인지 (1부터)
pub(super) fn line_of(text: &str, ci: usize) -> usize {
    text.chars().take(ci).filter(|&c| c == '\n').count() + 1
}

/// 선택한 글자를 링크 글자로 쓸 수 있게 다듬습니다 (첫 줄만, 마크다운 기호·대괄호 빼고, 너무 길면 자름).
pub(super) fn link_label(selected: &str) -> Option<String> {
    let first = selected.lines().map(str::trim).find(|l| !l.is_empty())?;
    let first = links::heading_text(first).unwrap_or(first);
    let cleaned: String = first.chars().filter(|c| !matches!(c, '[' | ']' | '*' | '`')).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let mut out: String = cleaned.chars().take(60).collect();
    if cleaned.chars().count() > 60 {
        out.push('…');
    }
    Some(out)
}

impl NotepadApp {
    pub(super) fn apply_edit_menu(&mut self, ctx: &egui::Context, id: Id, what: EditMenu, sel: Range<usize>) {
        let doc = &mut self.docs[self.active];
        let end = doc.text.chars().count();
        let sel = sel.start.min(end)..sel.end.min(end);
        let (a, b) = (markdown::char_to_byte(&doc.text, sel.start), markdown::char_to_byte(&doc.text, sel.end));
        let selected = doc.text[a..b].to_owned();
        // 메뉴를 누르는 동안 편집기 포커스가 빠져 선택이 풀렸을 수 있으므로 되살려 둠
        let restore = |ctx: &egui::Context, r: Range<usize>| {
            let mut state = TextEditState::load(ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(r.start), CCursor::new(r.end))));
            state.store(ctx, id);
        };
        match what {
            EditMenu::Copy => ctx.copy_text(selected),
            EditMenu::Cut => {
                ctx.copy_text(selected);
                doc.text.replace_range(a..b, "");
                doc.mark_changed();
                restore(ctx, sel.start..sel.start);
            }
            EditMenu::Paste => {
                let text = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok());
                match text.filter(|t| !t.is_empty()) {
                    Some(mut t) => {
                        if let Some(dir) = doc.dir()
                            && doc.is_markdown()
                            && let Some(rel) = links::relativize_links(&t, &dir, &self.settings.roots)
                        {
                            t = rel;
                        }
                        restore(ctx, sel);
                        self.pending_action = Some(MdAction::Insert(t));
                    }
                    // 글자가 없으면 그림 붙여넣기 (Ctrl+V 와 같게)
                    None => {
                        restore(ctx, sel);
                        self.paste_clipboard_image(ctx);
                    }
                }
            }
            EditMenu::SelectAll => self.pending_select = Some(0..end),
            EditMenu::CopyHyperlink => {
                let line = line_of(&doc.text, sel.start);
                let label = if sel.is_empty() { None } else { link_label(&selected) };
                self.copy_link_to_line(ctx, line, label);
                restore(ctx, sel);
            }
            EditMenu::MakeLink => {
                restore(ctx, sel);
                self.pending_action = Some(MdAction::Link);
            }
            EditMenu::NoteLink => {
                restore(ctx, sel);
                self.open_link_picker();
            }
        }
        self.focus_editor = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_lines() {
        assert_eq!(link_label("  **결정** 사항\n둘째 줄").as_deref(), Some("결정 사항"));
        assert_eq!(link_label("## 회의 [초안]").as_deref(), Some("회의 초안"));
        assert_eq!(link_label("  \n  "), None);
        assert!(link_label(&"가".repeat(80)).unwrap().ends_with('…'));
        assert_eq!(line_of("a\nb\nc", 0), 1);
        assert_eq!(line_of("a\nb\nc", 2), 2);
        assert_eq!(line_of("a\nb\nc", 5), 3);
    }
}
