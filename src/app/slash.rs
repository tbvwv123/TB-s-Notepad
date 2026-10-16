//! `/` 명령: 줄 맨 앞이나 띄어쓰기 뒤에서 `/` 를 치면 명령 목록이 뜹니다 (Notion 방식).
//! 이어서 글자를 치면 걸러지고(한글 초성도 됨), ↑↓ 로 고르고 Enter 로 실행합니다.
//! 스페이스를 치거나 Esc 를 누르면 목록이 닫히고 `/` 는 그냥 글자로 남습니다.
//! 같은 목록 창으로 `:` 커스텀 이모지도 고릅니다 (`:이름` 을 치면 맞는 이모지, `/이모지` 면 전부).

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Cmd {
    Heading(usize),
    Bullet,
    Numbered,
    Task,
    Quote,
    Toggle,
    Subtext,
    Highlight,
    CodeBlock,
    Table,
    Rule,
    WebLink,
    NoteLink,
    CopyLineLink,
    FileLink,
    FolderLink,
    Media,
    PasteImage,
    DateTime,
    Date,
    Emoji,
}

/// 목록 왼쪽의 작은 그림
#[derive(Clone, Copy)]
enum Badge {
    Icon(Icon),
    Text(&'static str),
}

pub(super) struct Command {
    pub cmd: Cmd,
    pub label: &'static str,
    /// 찾을 때 함께 보는 낱말 (영어 · 다른 이름)
    keywords: &'static str,
    pub hint: &'static str,
    badge: Badge,
}

const fn c(cmd: Cmd, label: &'static str, keywords: &'static str, hint: &'static str, badge: Badge) -> Command {
    Command { cmd, label, keywords, hint, badge }
}

pub(super) const COMMANDS: &[Command] = &[
    c(Cmd::Heading(1), "제목 1", "heading h1 큰제목 title", "Ctrl+Alt+1", Badge::Text("H1")),
    c(Cmd::Heading(2), "제목 2", "heading h2 중간제목", "Ctrl+Alt+2", Badge::Text("H2")),
    c(Cmd::Heading(3), "제목 3", "heading h3 작은제목", "Ctrl+Alt+3", Badge::Text("H3")),
    c(Cmd::Bullet, "글머리 목록", "bullet list 목록 리스트 점", "- ", Badge::Icon(Icon::Bullets)),
    c(Cmd::Numbered, "번호 목록", "numbered ordered list 숫자 목록", "1. ", Badge::Icon(Icon::Numbers)),
    c(Cmd::Task, "할 일", "todo task checkbox 체크 체크박스 할일", "- [ ] ", Badge::Icon(Icon::Task)),
    c(Cmd::Quote, "인용", "quote 인용문", "> ", Badge::Icon(Icon::Quote)),
    c(Cmd::Toggle, "토글", "toggle details collapse fold 접기 펼치기 토글목록", ">> ", Badge::Text("▸")),
    c(Cmd::Subtext, "작은 글씨", "subtext small 보조 설명", "-# ", Badge::Text("-#")),
    c(Cmd::Highlight, "형광펜", "highlight mark marker 강조 형광", "Ctrl+Shift+H", Badge::Text("==")),
    c(Cmd::CodeBlock, "코드 블록", "code block 코드", "```", Badge::Text("{ }")),
    c(Cmd::Table, "표", "table 표 칸", "", Badge::Icon(Icon::Table)),
    c(Cmd::Rule, "구분선", "divider rule hr line 선 가로선", "---", Badge::Icon(Icon::Rule)),
    c(Cmd::WebLink, "웹 링크", "link url web 링크 주소 하이퍼링크", "Ctrl+K", Badge::Icon(Icon::Link)),
    c(Cmd::NoteLink, "메모 링크", "note link page 메모 문서 링크 하이퍼링크", "Ctrl+Shift+K", Badge::Icon(Icon::Link)),
    c(Cmd::CopyLineLink, "이 줄 링크 복사", "copy line link 줄 복사 링크", "Ctrl+Shift+L", Badge::Text("#L")),
    c(Cmd::FileLink, "파일 링크", "file link explorer 파일 탐색기 링크", "", Badge::Icon(Icon::Folder)),
    c(Cmd::FolderLink, "폴더 링크", "folder link explorer 폴더 탐색기 링크", "", Badge::Icon(Icon::Folder)),
    c(
        Cmd::Media,
        "그림 · 소리 · 영상",
        "image picture photo audio music video media 그림 사진 이미지 소리 음악 영상 동영상",
        "",
        Badge::Icon(Icon::Image),
    ),
    c(
        Cmd::PasteImage,
        "클립보드 그림 붙여넣기",
        "paste clipboard image screenshot 붙여넣기 캡처",
        "Ctrl+V",
        Badge::Icon(Icon::Image),
    ),
    c(Cmd::DateTime, "날짜와 시간", "date time now 지금 시각 날짜", "F5", Badge::Text("12")),
    c(Cmd::Date, "오늘 날짜", "today date 오늘 날짜", "", Badge::Text("31")),
    c(Cmd::Emoji, "이모지", "emoji sticker custom 이모티콘 스티커 커스텀", ":", Badge::Text(":)")),
];

const CHOSUNG: [char; 19] =
    ['ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ'];

/// 한글 글자의 첫소리만 (`메모 링크` → `ㅁㅁㄹㅋ`)
fn chosung(s: &str) -> String {
    s.chars()
        .filter_map(|ch| {
            let u = ch as u32;
            if (0xAC00..=0xD7A3).contains(&u) {
                Some(CHOSUNG[((u - 0xAC00) / 588) as usize])
            } else if ch.is_whitespace() || ch == '·' {
                None
            } else {
                Some(ch)
            }
        })
        .collect()
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '·').flat_map(char::to_lowercase).collect()
}

/// 이 명령이 `query` 에 맞으면 순위 (작을수록 위)
fn rank(cmd: &Command, query: &str) -> Option<u8> {
    let q = squash(query);
    if q.is_empty() {
        return Some(0);
    }
    // 지금 언어의 이름과 한국어 이름 둘 다로 찾음
    let label = squash(crate::i18n::tr(cmd.label));
    let label_ko = squash(cmd.label);
    if label.starts_with(&q) || label_ko.starts_with(&q) {
        return Some(0);
    }
    if cmd.keywords.split_whitespace().any(|k| k.to_lowercase().starts_with(&q)) {
        return Some(1);
    }
    if label.contains(&q) || label_ko.contains(&q) {
        return Some(2);
    }
    if q.chars().all(|c| CHOSUNG.contains(&c)) && chosung(cmd.label).contains(&q) {
        return Some(3);
    }
    None
}

/// `query` 에 맞는 명령들의 번호 (맞는 순서대로)
pub(super) fn filter(query: &str) -> Vec<usize> {
    let mut hits: Vec<(u8, usize)> =
        COMMANDS.iter().enumerate().filter_map(|(i, c)| rank(c, query).map(|r| (r, i))).collect();
    hits.sort();
    hits.into_iter().map(|(_, i)| i).collect()
}

/// 방금 커서 바로 앞에 친 `trigger`(`/` 나 `:`) 가 목록을 여는 자리인지 (줄 맨 앞이나 띄어쓰기 뒤).
/// 그렇다면 그 글자의 위치.
fn trigger_start(text: &str, cursor: usize, trigger: char) -> Option<usize> {
    let start = cursor.checked_sub(1)?;
    let mut before = text.chars().skip(start.saturating_sub(1));
    let prev = if start == 0 { None } else { before.next() };
    let ch = if start == 0 { text.chars().next() } else { before.next() };
    // 이모지는 띄어 쓰지 않고 이어 넣을 수 있게, 이모지 바로 뒤의 `:` 도 엶
    let after_emoji = || {
        trigger == ':'
            && text[..markdown::char_to_byte(text, start)].ends_with(&format!("\"{}\")", crate::emoji::TITLE))
    };
    (ch == Some(trigger) && (prev.is_none_or(char::is_whitespace) || after_emoji())).then_some(start)
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
    Emoji(usize),
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
}

impl SlashMenu {
    fn new(doc_id: u64, start: usize, kind: MenuKind, explicit: bool, emoji: Vec<crate::emoji::Emoji>) -> Self {
        let shown = kind == MenuKind::Commands || explicit;
        Self { doc_id, start, selected: 0, scroll: false, popup: None, kind, explicit, emoji, shown }
    }

    fn items(&self, query: &str) -> Vec<Item> {
        match self.kind {
            MenuKind::Commands => filter(query).into_iter().map(Item::Cmd).collect(),
            MenuKind::Emoji => {
                let mut v: Vec<Item> =
                    crate::emoji::filter(&self.emoji, query).into_iter().take(50).map(Item::Emoji).collect();
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
        match self.kind {
            MenuKind::Commands => true,
            MenuKind::Emoji => {
                self.explicit || (!query.is_empty() && items.iter().any(|i| matches!(i, Item::Emoji(_))))
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
        let cursor = TextEditState::load(ctx, id)?.cursor.char_range()?.primary.index.0;
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
    /// `typed`: 이번 프레임에 친 `/` 나 `:`
    pub(super) fn slash_menu(
        &mut self,
        ctx: &egui::Context,
        id: Id,
        typed: Option<char>,
        caret: Option<egui::Rect>,
        focused: bool,
        key: Option<MenuKey>,
    ) {
        let doc_id = self.doc().id;
        let cursor = TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0);
        if !self.doc().is_markdown() {
            self.slash = None;
            return;
        }
        // 새로 열기 (`:` 는 이 메모 폴더에 이모지가 있을 때만)
        if let Some(ch) = typed
            && focused
            && let Some(c) = cursor
            && let Some(start) = trigger_start(&self.doc().text, c, ch)
        {
            if ch == '/' {
                self.slash = Some(SlashMenu::new(doc_id, start, MenuKind::Commands, false, Vec::new()));
            } else {
                let emoji = self.emoji_list();
                if !emoji.is_empty() {
                    self.slash = Some(SlashMenu::new(doc_id, start, MenuKind::Emoji, false, emoji));
                }
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
        if menu.doc_id != doc_id || key == Some(MenuKey::Close) || (!focused && !pointer_on_popup) {
            self.slash = None;
            return;
        }
        // `:이름:` 을 끝까지 치면 바로 그 이모지로 (맞는 이름이 없으면 그냥 글자로 두고 닫음)
        if menu.kind == MenuKind::Emoji && query.contains(':') {
            let start = menu.start;
            let hit = query
                .strip_suffix(':')
                .and_then(|name| menu.emoji.iter().find(|e| e.name.eq_ignore_ascii_case(name)))
                .cloned();
            self.slash = None;
            if let Some(e) = hit {
                self.insert_emoji(ctx, id, start..cursor, &e);
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
            Item::AddEmoji => {
                self.remove_typed(ctx, id, start..cursor);
                self.add_emoji_dialog(&query, Some(start));
            }
        }
    }

    /// 목록을 열려고 친 글자(`/검색어`)를 지우고 커서를 그 자리에
    fn remove_typed(&mut self, ctx: &egui::Context, id: Id, typed: Range<usize>) {
        let doc = &mut self.docs[self.active];
        let (a, b) = (markdown::char_to_byte(&doc.text, typed.start), markdown::char_to_byte(&doc.text, typed.end));
        doc.text.replace_range(a..b, "");
        doc.mark_changed();
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(typed.start))));
        state.store(ctx, id);
        self.focus_editor = true;
    }

    /// `/검색어` 글자를 지우고 명령을 실행합니다.
    fn run_slash(&mut self, ctx: &egui::Context, id: Id, typed: Range<usize>, cmd: Cmd) {
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
                let mut state = TextEditState::load(ctx, id).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at + 1))));
                state.store(ctx, id);
                let emoji = self.emoji_list();
                self.slash = Some(SlashMenu::new(self.doc().id, at, MenuKind::Emoji, true, emoji));
                None
            }
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
            let range = if line_prefix {
                CCursorRange::one(CCursor::new(shifted))
            } else {
                CCursorRange::two(CCursor::new(sel.start), CCursor::new(sel.end))
            };
            let mut state = TextEditState::load(ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(range));
            state.store(ctx, id);
        }
        ctx.request_repaint();
    }
}

/// 목록 창. 누른 줄과 창 자리를 돌려줍니다.
fn popup_ui(
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
                                if let Ok(egui::load::TexturePoll::Ready { texture }) =
                                    ctx.try_load_texture(&uri, egui::TextureOptions::LINEAR, egui::SizeHint::default())
                                {
                                    let s = texture.size;
                                    let scale = if s.x > 0.0 && s.y > 0.0 { (24.0 / s.x).min(24.0 / s.y) } else { 1.0 };
                                    let r = egui::Rect::from_center_size(badge.center(), s * scale);
                                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                                    p.image(texture.id, r, uv, Color32::WHITE);
                                }
                                (format!(":{}:", e.name), "")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(q: &str) -> Vec<&'static str> {
        filter(q).into_iter().map(|i| COMMANDS[i].label).collect()
    }

    #[test]
    fn filters_by_label_keyword_and_chosung() {
        assert_eq!(labels("").len(), COMMANDS.len());
        assert_eq!(labels("제목")[..3], ["제목 1", "제목 2", "제목 3"]);
        assert_eq!(labels("h2"), ["제목 2"]);
        assert_eq!(labels("메모링크")[0], "메모 링크");
        assert_eq!(labels("메모 링크")[0], "메모 링크");
        assert_eq!(labels("ㅁㅁㄹㅋ"), ["메모 링크"]);
        assert!(labels("링크").contains(&"웹 링크") && labels("링크").contains(&"파일 링크"));
        assert!(labels("video").contains(&"그림 · 소리 · 영상"));
        assert!(labels("todo").contains(&"할 일"));
        assert!(labels("없는명령").is_empty());
    }

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
        let after = "![:a:](a.png \"emoji\"):";
        assert_eq!(trigger_start(after, after.chars().count(), ':'), Some(after.chars().count() - 1), "이모지 바로 뒤");
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
