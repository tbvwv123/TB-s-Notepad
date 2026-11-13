//! `/` 명령: 줄 맨 앞이나 띄어쓰기 뒤에서 `/` 를 치면 명령 목록이 뜹니다 (Notion 방식).
//! 이어서 글자를 치면 걸러지고(한글 초성도 됨), ↑↓ 로 고르고 Enter 로 실행합니다.
//! 스페이스를 치거나 Esc 를 누르면 목록이 닫히고 `/` 는 그냥 글자로 남습니다 (Esc 를 눌러도 커서는 그 자리에 그대로).
//! 같은 목록 창으로 `:` 커스텀 이모지도 고릅니다 (`:이름` 을 치면 맞는 이모지, `/이모지` 면 전부).

use super::*;

mod commands;
mod popup;
mod run;

use commands::{COMMANDS, filter};
use popup::popup_ui;

/// 방금 커서 바로 앞에 친 `trigger`(`/` 나 `:`) 가 목록을 여는 자리인지 (줄 맨 앞이나 띄어쓰기 뒤).
/// 그렇다면 그 글자의 위치.
fn trigger_start(text: &str, cursor: usize, trigger: char) -> Option<usize> {
    let start = cursor.checked_sub(1)?;
    let mut before = text.chars().skip(start.saturating_sub(1));
    let prev = if start == 0 { None } else { before.next() };
    let ch = if start == 0 { text.chars().next() } else { before.next() };
    // 이모지는 띄어 쓰지 않고 이어 넣을 수 있게, 이모지 바로 뒤의 `:` 도 엶
    // 커스텀 이모지나 기본 이모지 바로 뒤 (이모지를 띄어 쓰지 않고 이어 넣을 수 있게)
    let after_emoji = || {
        let before = &text[..markdown::char_to_byte(text, start)];
        trigger == ':'
            && (before.ends_with(&format!("\"{}\")", crate::emoji::TITLE))
                || prev.is_some_and(|c| c == '\u{fe0f}' || crate::twemoji::at(c.encode_utf8(&mut [0; 4])).is_some()))
    };
    // 토글 제목 맨 앞 (`<summary>` 바로 뒤)
    let title_start = || text[..markdown::char_to_byte(text, start)].ends_with("<summary>");
    (ch == Some(trigger) && (prev.is_none_or(char::is_whitespace) || after_emoji() || title_start())).then_some(start)
}

/// 커서 앞의 낱말이 `/` 나 `:` 로 시작해 목록을 열 자리면 (그 글자, 위치).
/// 한 프레임에 여러 글자가 함께 들어와도 (`:thu`) 열리도록 커서 바로 앞 글자만 보지 않고 낱말 처음을 봅니다.
fn word_trigger(text: &str, cursor: usize) -> Option<(char, usize)> {
    let byte = markdown::char_to_byte(text, cursor);
    let word_start = text[..byte].rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let mut word = &text[word_start..byte];
    // 토글 제목 맨 앞 (`<summary>` 바로 뒤)이나 이모지 바로 뒤도 낱말 처음으로 봄
    for prefix in ["<summary>", "\"emoji\")"] {
        if let Some(rest) = word.strip_prefix(prefix) {
            word = rest;
        }
    }
    while let Some((_, len)) = crate::twemoji::at(word) {
        word = &word[len..];
    }
    let n = word.chars().count();
    let first = word.chars().next()?;
    if !matches!(first, '/' | ':') || n > 33 {
        return None;
    }
    let start = cursor - n;
    trigger_start(text, start + 1, first).map(|s| (first, s))
}

/// 열려 있는 목록의 지금 검색어. 여는 글자가 지워졌거나 띄어쓰기가 들어가면 `None` (목록 닫기).
fn query_at(text: &str, start: usize, cursor: usize, trigger: char) -> Option<String> {
    if cursor <= start {
        return None;
    }
    let mut it = text.chars().skip(start);
    if it.next() != Some(trigger) {
        return None;
    }
    let q: String = it.take(cursor - start - 1).collect();
    (q.chars().count() == cursor - start - 1 && !q.chars().any(char::is_whitespace) && q.chars().count() <= 33)
        .then_some(q)
}

/// 목록 종류: `/` 명령 또는 `:` 커스텀 이모지
#[derive(Clone, Copy, PartialEq)]
pub(super) enum MenuKind {
    Commands,
    Emoji,
}

impl MenuKind {
    fn trigger(self) -> char {
        match self {
            MenuKind::Commands => '/',
            MenuKind::Emoji => ':',
        }
    }
}

/// 목록의 한 줄
#[derive(Clone, Copy, PartialEq)]
enum Item {
    Cmd(usize),
    /// 이 메모 폴더의 커스텀 이모지
    Emoji(usize),
    /// 기본 이모지 (트위모지)
    Builtin(usize),
    /// 그림 파일을 골라 새 이모지 등록
    AddEmoji,
}

/// 열려 있는 `/` 명령 · `:` 이모지 목록
pub(super) struct SlashMenu {
    doc_id: u64,
    /// `/` · `:` 의 글자 위치
    start: usize,
    selected: usize,
    /// 키보드로 고른 줄을 보이게 스크롤
    scroll: bool,
    /// 지난 프레임에 그린 목록 자리 (목록을 누르는 동안 편집기 포커스가 빠져도 닫지 않게)
    popup: Option<egui::Rect>,
    kind: MenuKind,
    /// `/이모지` 로 연 이모지 목록: 검색어가 없어도 보이고 "새 이모지 추가" 줄이 있음
    explicit: bool,
    /// 이 메모 폴더의 이모지 (목록을 열 때 읽어 둠)
    emoji: Vec<crate::emoji::Emoji>,
    /// 지난 프레임에 목록이 보였는지 (`:` 목록은 맞는 이모지가 있을 때만 보임)
    shown: bool,
    /// Esc 로 닫음: 커서가 이 낱말을 떠날 때까지 목록을 다시 열지 않음
    /// (닫은 뒤 `/usr` 처럼 이어 쓰는 글자마다 목록이 다시 뜨지 않게)
    dismissed: bool,
}

impl SlashMenu {
    fn new(doc_id: u64, start: usize, kind: MenuKind, explicit: bool, emoji: Vec<crate::emoji::Emoji>) -> Self {
        let shown = kind == MenuKind::Commands || explicit;
        Self { doc_id, start, selected: 0, scroll: false, popup: None, kind, explicit, emoji, shown, dismissed: false }
    }

    fn items(&self, query: &str) -> Vec<Item> {
        match self.kind {
            MenuKind::Commands => filter(query).into_iter().map(Item::Cmd).collect(),
            MenuKind::Emoji => {
                // 커스텀 이모지가 먼저, 그다음 기본 이모지 (검색어가 없으면 기본 이모지는 앞쪽 몇 개만)
                let mut v: Vec<Item> =
                    crate::emoji::filter(&self.emoji, query).into_iter().take(50).map(Item::Emoji).collect();
                let room = if query.is_empty() { 24usize } else { 60 }.saturating_sub(v.len());
                v.extend(crate::twemoji::filter(query).into_iter().take(room).map(Item::Builtin));
                if self.explicit || !v.is_empty() {
                    v.push(Item::AddEmoji);
                }
                v
            }
        }
    }

    /// 목록을 보여 줄지: 명령은 늘, 이모지는 `/이모지` 로 열었거나 한 글자 이상 쳐서 맞는 이모지가 있을 때
    /// (`:` 는 "시간: 3시" 처럼 그냥 글에도 흔하므로)
    fn visible(&self, query: &str, items: &[Item]) -> bool {
        !self.dismissed
            && match self.kind {
                MenuKind::Commands => true,
                MenuKind::Emoji => {
                    self.explicit
                        || (!query.is_empty() && items.iter().any(|i| matches!(i, Item::Emoji(_) | Item::Builtin(_))))
                }
            }
    }
}

/// 목록이 열려 있을 때 편집기 대신 받는 키
#[derive(Clone, Copy, PartialEq)]
pub(super) enum MenuKey {
    Up,
    Down,
    Run,
    Close,
}

impl NotepadApp {
    pub(super) fn slash_is_open(&self) -> bool {
        self.slash.as_ref().is_some_and(|m| m.doc_id == self.doc().id && m.shown)
    }

    fn slash_query(&self, ctx: &egui::Context, id: Id) -> Option<String> {
        let menu = self.slash.as_ref()?;
        let doc = self.doc();
        if menu.doc_id != doc.id {
            return None;
        }
        let cursor = cursor_index(ctx, id)?;
        query_at(&doc.text, menu.start, cursor, menu.kind.trigger())
    }

    /// TextEdit 보다 먼저: 목록이 보이면 ↑↓ · Enter · Tab · Esc 를 가로챕니다.
    pub(super) fn slash_keys(&mut self, ctx: &egui::Context, id: Id) -> Option<MenuKey> {
        let query = self.slash_query(ctx, id)?;
        let menu = self.slash.as_ref()?;
        let items = menu.items(&query);
        if !menu.visible(&query, &items) {
            return None;
        }
        let has_matches = !items.is_empty();
        ctx.input_mut(|i| {
            let mut take = |k: Key| i.consume_key(Modifiers::NONE, k);
            if take(Key::Escape) {
                Some(MenuKey::Close)
            } else if take(Key::ArrowDown) {
                Some(MenuKey::Down)
            } else if take(Key::ArrowUp) {
                Some(MenuKey::Up)
            } else if has_matches && (take(Key::Enter) || take(Key::Tab)) {
                Some(MenuKey::Run)
            } else {
                None
            }
        })
    }

    /// TextEdit 다음: 목록을 열고 닫고, 그리고, 고른 것을 실행합니다.
    /// `typed`: 이번 프레임에 글자를 쳤는지
    pub(super) fn slash_menu(
        &mut self,
        ctx: &egui::Context,
        id: Id,
        typed: bool,
        caret: Option<egui::Rect>,
        focused: bool,
        key: Option<MenuKey>,
    ) {
        let doc_id = self.doc().id;
        let cursor = cursor_index(ctx, id);
        if !self.doc().is_markdown() {
            self.slash = None;
            return;
        }
        // 새로 열기 (커서 앞 낱말이 `/` · `:` 로 시작할 때, 또는 이모지 바로 뒤에 `:` 를 쳤을 때)
        if typed
            && focused
            && self.slash.as_ref().is_none_or(|m| m.doc_id != doc_id)
            && let Some(c) = cursor
            && let Some((ch, start)) = word_trigger(&self.doc().text, c)
                .or_else(|| ['/', ':'].into_iter().find_map(|ch| Some((ch, trigger_start(&self.doc().text, c, ch)?))))
        {
            if ch == '/' {
                self.slash = Some(SlashMenu::new(doc_id, start, MenuKind::Commands, false, Vec::new()));
            } else {
                let emoji = self.emoji_list();
                self.slash = Some(SlashMenu::new(doc_id, start, MenuKind::Emoji, false, emoji));
            }
        }
        let Some(menu) = &mut self.slash else { return };
        let pointer_on_popup =
            menu.popup.is_some_and(|r| ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p)));
        let query = cursor.and_then(|c| query_at(&self.docs[self.active].text, menu.start, c, menu.kind.trigger()));
        let (Some(query), Some(cursor)) = (query, cursor) else {
            self.slash = None;
            return;
        };
        if menu.doc_id != doc_id || (!focused && !pointer_on_popup) {
            self.slash = None;
            return;
        }
        // Esc: 목록만 닫고 친 글자와 커서는 그대로. 커서가 이 낱말을 떠나면 (위의 `query_at`) 완전히 잊음
        if key == Some(MenuKey::Close) {
            menu.dismissed = true;
        }
        if menu.dismissed {
            menu.shown = false;
            menu.popup = None;
            return;
        }
        // `:이름:` 을 끝까지 치면 바로 그 이모지로 (맞는 이름이 없으면 그냥 글자로 두고 닫음)
        if menu.kind == MenuKind::Emoji && query.contains(':') {
            let start = menu.start;
            let name = query.strip_suffix(':');
            let hit = name.and_then(|name| menu.emoji.iter().find(|e| e.name.eq_ignore_ascii_case(name))).cloned();
            let builtin = name.and_then(crate::twemoji::find);
            self.slash = None;
            if let Some(e) = hit {
                self.insert_emoji(ctx, id, start..cursor, &e);
            } else if let Some(i) = builtin {
                self.replace_typed(ctx, id, start..cursor, crate::twemoji::TWEMOJI[i].1);
            }
            return;
        }
        let items = menu.items(&query);
        menu.shown = menu.visible(&query, &items);
        if !menu.shown {
            menu.popup = None;
            return;
        }
        menu.selected = menu.selected.min(items.len().saturating_sub(1));
        match key {
            Some(MenuKey::Down) if !items.is_empty() => {
                menu.selected = (menu.selected + 1) % items.len();
                menu.scroll = true;
            }
            Some(MenuKey::Up) if !items.is_empty() => {
                menu.selected = (menu.selected + items.len() - 1) % items.len();
                menu.scroll = true;
            }
            _ => {}
        }
        let mut run = (key == Some(MenuKey::Run)).then(|| items.get(menu.selected).copied()).flatten();

        // 목록 그리기 (커서 바로 아래, 아래가 모자라면 위)
        if run.is_none()
            && let Some(caret) = caret
        {
            let (clicked, rect) = popup_ui(ctx, menu, caret, &items, &query);
            menu.popup = Some(rect);
            if clicked.is_some() {
                run = clicked;
            }
        }
        let Some(item) = run else { return };
        let start = menu.start;
        let emoji = match item {
            Item::Emoji(j) => menu.emoji.get(j).cloned(),
            _ => None,
        };
        self.slash = None;
        match item {
            Item::Cmd(i) => self.run_slash(ctx, id, start..cursor, COMMANDS[i].cmd),
            Item::Emoji(_) => {
                if let Some(e) = emoji {
                    self.insert_emoji(ctx, id, start..cursor, &e);
                }
            }
            Item::Builtin(i) => self.replace_typed(ctx, id, start..cursor, crate::twemoji::TWEMOJI[i].1),
            Item::AddEmoji => {
                self.remove_typed(ctx, id, start..cursor);
                self.add_emoji_dialog(&query, Some(start));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slash_opens_only_at_word_start() {
        let slash = |t, c| trigger_start(t, c, '/');
        assert_eq!(slash("/", 1), Some(0));
        assert_eq!(slash("글 /", 3), Some(2));
        assert_eq!(slash("한\n/", 3), Some(2));
        assert_eq!(slash("10/", 3), None, "날짜·경로 속 / 는 그냥 글자");
        assert_eq!(slash("http:/", 6), None);
        assert_eq!(slash("a", 1), None);
        assert_eq!(slash("", 0), None);
        // 이모지 `:` 도 같은 규칙
        assert_eq!(trigger_start("오늘 :", 4, ':'), Some(3));
        assert_eq!(trigger_start("시간:", 3, ':'), None, "낱말 뒤의 : 는 그냥 글자");
        assert_eq!(trigger_start("글 /", 3, ':'), None);
        assert_eq!(trigger_start("<summary>/", 10, '/'), Some(9), "토글 제목 맨 앞");
        let after = "![:a:](a.png \"emoji\"):";
        assert_eq!(trigger_start(after, after.chars().count(), ':'), Some(after.chars().count() - 1), "이모지 바로 뒤");
    }

    #[test]
    fn opens_even_when_several_chars_arrive_at_once() {
        assert_eq!(word_trigger(":thu", 4), Some((':', 0)));
        assert_eq!(word_trigger("글 /표", 4), Some(('/', 2)));
        assert_eq!(word_trigger("시간:3", 4), None, "낱말 가운데의 : 는 아님");
        assert_eq!(word_trigger("a/b", 3), None);
        assert_eq!(word_trigger("<summary>:ta", 12), Some((':', 9)), "토글 제목 맨 앞");
        assert_eq!(word_trigger("👍❤️:ta", 6), Some((':', 3)), "기본 이모지 바로 뒤");
    }

    #[test]
    fn esc_keeps_the_menu_closed_while_typing_on() {
        let mut menu = SlashMenu::new(1, 0, MenuKind::Commands, false, Vec::new());
        let items = menu.items("");
        assert!(menu.visible("", &items));
        menu.dismissed = true;
        assert!(!menu.visible("", &items) && !menu.visible("제목", &menu.items("제목")), "이어 써도 다시 뜨지 않음");
    }

    #[test]
    fn query_closes_on_space_or_deleted_slash() {
        let t = "글 /메모 끝";
        let q = |t, s, c| query_at(t, s, c, '/');
        assert_eq!(q(t, 2, 3).as_deref(), Some(""));
        assert_eq!(q(t, 2, 5).as_deref(), Some("메모"));
        assert_eq!(q(t, 2, 6), None, "스페이스를 치면 닫힘");
        assert_eq!(q(t, 2, 2), None, "/ 앞으로 커서가 가면 닫힘");
        assert_eq!(q("글 메모", 2, 3), None, "/ 가 지워지면 닫힘");
        assert_eq!(query_at("글 :party:", 2, 9, ':').as_deref(), Some("party:"));
    }
}
