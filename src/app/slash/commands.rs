//! `/` 명령 표와 걸러내기 (이름 · 다른 이름 · 한글 초성).

use super::super::*;

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
pub(super) enum Badge {
    Icon(Icon),
    Text(&'static str),
}

pub(super) struct Command {
    pub cmd: Cmd,
    pub label: &'static str,
    /// 찾을 때 함께 보는 낱말 (영어 · 다른 이름)
    keywords: &'static str,
    pub hint: &'static str,
    pub badge: Badge,
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
}
