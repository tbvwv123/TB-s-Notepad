//! 입력 도우미: 목록 이어 쓰기(Enter), 들여쓰기·내어쓰기(Tab / Shift+Tab), 토글 만들기·접기.
//!
//! 모두 (글자, 커서) 를 받아 바뀐 글자와 새 커서를 돌려주는 순수 함수라 편집기 밖에서도 시험할 수 있습니다.
//! 커서·범위는 TextEdit 과 같은 "글자 단위" 위치입니다.

use std::ops::Range;

use crate::markdown::{byte_to_char, char_to_byte};

/// 들여쓰기 한 단계
const INDENT: &str = "  ";

/// 줄 앞의 목록 기호: (들여쓰기, 기호 원문, 다음 줄에 쓸 기호)
fn list_prefix(line: &str) -> Option<(&str, &str, String)> {
    let body = line.trim_start_matches([' ', '\t']);
    let indent = &line[..line.len() - body.len()];
    let b = body.as_bytes();
    // - [ ] 할 일 / - [x] 끝낸 일
    if b.len() >= 6 && matches!(b[0], b'-' | b'*' | b'+') && b[1..3] == *b" [" && b[4] == b']' && b[5] == b' ' {
        let bullet = &body[..1];
        return Some((indent, &body[..6], format!("{bullet} [ ] ")));
    }
    // - 항목
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return Some((indent, &body[..2], body[..2].to_owned()));
    }
    // 1. 항목 / 1) 항목
    let d = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&d) && b.len() > d + 1 && matches!(b[d], b'.' | b')') && b[d + 1] == b' ' {
        let n: u64 = body[..d].parse().ok()?;
        return Some((indent, &body[..d + 2], format!("{}{} ", n + 1, b[d] as char)));
    }
    // > 인용
    if body.starts_with("> ") {
        return Some((indent, &body[..2], "> ".to_owned()));
    }
    None
}

/// 커서가 있는 줄의 바이트 범위 (줄바꿈 제외)
fn line_at(text: &str, byte: usize) -> Range<usize> {
    let start = text[..byte].rfind('\n').map_or(0, |i| i + 1);
    let end = text[byte..].find('\n').map_or(text.len(), |i| byte + i);
    start..end
}

/// 목록·할 일·번호·인용 줄에서 Enter: 다음 줄에 같은 기호를 이어 붙입니다.
/// 내용 없는 항목에서 Enter 를 치면 들여쓴 항목은 한 단계 내어 쓰고, 아니면 기호를 지워 목록을 끝냅니다.
/// 목록 줄이 아니거나 커서가 기호 안쪽이면 `None` (보통 줄바꿈).
pub fn enter_in_list(text: &str, cursor: usize) -> Option<(String, usize)> {
    let byte = char_to_byte(text, cursor);
    let line = line_at(text, byte);
    let (indent, marker, next) = list_prefix(&text[line.clone()])?;
    let content_start = line.start + indent.len() + marker.len();
    if byte < content_start {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 8);
    if text[content_start..line.end].trim().is_empty() {
        // 빈 항목: 내어쓰기 또는 목록 끝내기
        let replacement = match indent.strip_prefix(INDENT).or_else(|| indent.strip_prefix('\t')) {
            Some(less) => format!("{less}{marker}"),
            None => String::new(),
        };
        out.push_str(&text[..line.start]);
        out.push_str(&replacement);
        let new_cursor = byte_to_char(&out, out.len());
        out.push_str(&text[line.end..]);
        return Some((out, new_cursor));
    }
    out.push_str(&text[..byte]);
    out.push('\n');
    out.push_str(indent);
    out.push_str(&next);
    let new_cursor = byte_to_char(&out, out.len());
    out.push_str(&text[byte..]);
    Some((out, new_cursor))
}

/// 고른 줄들(또는 커서 줄)이 목록이면 Tab 은 들여쓰기, Shift+Tab 은 내어쓰기. 목록 줄이 없으면 `None`.
pub fn indent_list(text: &str, sel: Range<usize>, outdent: bool) -> Option<(String, Range<usize>)> {
    let (a, b) = (char_to_byte(text, sel.start), char_to_byte(text, sel.end));
    let first = line_at(text, a).start;
    let last = line_at(text, b).end;
    let lines: Vec<&str> = text[first..last].split('\n').collect();
    if !lines.iter().any(|l| list_prefix(l).is_some()) {
        return None;
    }
    let mut shift_start: isize = 0; // 첫 줄에서 커서가 움직인 글자 수
    let mut shift_total: isize = 0;
    let new_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let delta: isize = if list_prefix(l).is_none() {
                0
            } else if outdent {
                let removed = if l.starts_with(INDENT) {
                    2
                } else if l.starts_with([' ', '\t']) {
                    1
                } else {
                    0
                };
                -removed
            } else {
                2
            };
            if i == 0 {
                shift_start = delta;
            }
            shift_total += delta;
            match delta {
                d if d > 0 => format!("{INDENT}{l}"),
                d if d < 0 => l[(-d) as usize..].to_owned(),
                _ => (*l).to_owned(),
            }
        })
        .collect();
    let mut out = String::with_capacity(text.len() + 16);
    out.push_str(&text[..first]);
    out.push_str(&new_lines.join("\n"));
    out.push_str(&text[last..]);
    let start = (sel.start as isize + shift_start).max(byte_to_char(text, first) as isize) as usize;
    let end = (sel.end as isize + shift_total).max(start as isize) as usize;
    Some((out, start..end))
}

// ---------------------------------------------------------------------------
// 토글 (접었다 펴는 블록). 파일에는 GitHub · VS Code 에서도 접히는 HTML 로 저장합니다:
//
//     <details open>
//     <summary>제목</summary>
//
//     내용
//
//     </details>

pub const TOGGLE_CLOSE: &str = "</details>";

/// `<details>` 여는 줄이면 펼쳐져 있는지 (`open` 속성)
pub fn toggle_open_line(line: &str) -> Option<bool> {
    let t = line.trim();
    let inner = t.strip_prefix("<details")?.strip_suffix('>')?;
    if !(inner.is_empty() || inner.starts_with(' ')) {
        return None;
    }
    Some(inner.split_whitespace().any(|a| a == "open" || a.starts_with("open=")))
}

/// `<summary>제목</summary>` 줄이면 제목의 바이트 범위 (줄 기준)
pub fn summary_line(line: &str) -> Option<Range<usize>> {
    let lead = line.len() - line.trim_start().len();
    let t = line.trim();
    let inner = t.strip_prefix("<summary>")?.strip_suffix("</summary>")?;
    let start = lead + "<summary>".len();
    Some(start..start + inner.len())
}

pub fn is_toggle_close(line: &str) -> bool {
    line.trim() == TOGGLE_CLOSE
}

/// 줄을 토글로 바꿉니다: 그 줄의 글자가 제목이 되고, 커서는 제목 끝에 둡니다.
/// (`>> ` 를 쳤을 때, `/토글`, 서식 메뉴에서)
pub fn make_toggle(text: &str, cursor: usize) -> (String, usize) {
    let byte = char_to_byte(text, cursor);
    let line = line_at(text, byte);
    let raw = &text[line.clone()];
    let indent = &raw[..raw.len() - raw.trim_start().len()];
    let title = raw.trim().trim_start_matches(">>").trim();
    let mut out = String::with_capacity(text.len() + 64);
    out.push_str(&text[..line.start]);
    out.push_str(&format!("{indent}<details open>\n{indent}<summary>{title}"));
    let new_cursor = byte_to_char(&out, out.len());
    out.push_str(&format!("</summary>\n\n\n\n{indent}{TOGGLE_CLOSE}"));
    out.push_str(&text[line.end..]);
    (out, new_cursor)
}

/// 커서가 토글 제목 줄에 있으면 그 토글의 `<details>` 줄 시작 바이트
pub fn toggle_at_cursor(text: &str, cursor: usize) -> Option<usize> {
    let byte = char_to_byte(text, cursor);
    let line = line_at(text, byte);
    if summary_line(&text[line.clone()]).is_none() || line.start == 0 {
        return None;
    }
    let prev = line_at(text, line.start - 1);
    toggle_open_line(&text[prev.clone()]).map(|_| prev.start)
}

/// 토글 제목에서 Enter (Notion 처럼): 제목은 한 줄 그대로 두고, 커서 뒤의 글자를 데리고 토글 안 첫 줄로 내려갑니다.
/// 접혀 있으면 폅니다. 제목 줄이 아니면 `None`.
/// (제목 안에 줄바꿈이 들어가면 `<summary>` 가 여러 줄로 나뉘어 토글이 깨지므로)
pub fn enter_in_summary(text: &str, cursor: usize) -> Option<(String, usize)> {
    let details = toggle_at_cursor(text, cursor)?;
    let mut text = text.to_owned();
    let mut cursor = cursor;
    if set_toggle_open(&mut text, details, true) {
        cursor += " open".len();
    }
    let byte = char_to_byte(&text, cursor);
    let line = line_at(&text, byte);
    let title = summary_line(&text[line.clone()])?;
    let (t0, t1) = (line.start + title.start, line.start + title.end);
    let byte = byte.clamp(t0, t1);
    let raw = &text[line.clone()];
    let indent = &raw[..raw.len() - raw.trim_start().len()];
    let rest = text[byte..t1].trim_start();

    let mut out = String::with_capacity(text.len() + indent.len() + 4);
    out.push_str(&text[..byte]);
    out.push_str("</summary>");
    // 제목 다음은 빈 줄, 그다음이 내용 첫 줄 (`make_toggle` 이 만드는 모양)
    let l1 = (line.end < text.len()).then(|| line_at(&text, line.end + 1));
    let blank = |r: &Range<usize>| text[r.clone()].trim().is_empty();
    let l2 = l1.as_ref().filter(|r| blank(r) && r.end < text.len()).map(|r| line_at(&text, r.end + 1));
    let new_cursor;
    match (l1, l2) {
        // 내용 첫 줄이 비어 있으면 그 줄에
        (Some(_), Some(l2)) if blank(&l2) && !is_toggle_close(&text[l2.clone()]) => {
            out.push_str(&text[line.end..l2.start]);
            out.push_str(indent);
            new_cursor = byte_to_char(&out, out.len());
            out.push_str(rest);
            out.push_str(&text[l2.end..]);
        }
        // 빈 줄 다음에 새 줄을 끼워서
        (Some(l1), _) if blank(&l1) => {
            out.push_str(&text[line.end..l1.end]);
            out.push('\n');
            out.push_str(indent);
            new_cursor = byte_to_char(&out, out.len());
            out.push_str(rest);
            out.push_str(&text[l1.end..]);
        }
        // 빈 줄이 없으면 빈 줄과 새 줄을 함께
        _ => {
            out.push_str("\n\n");
            out.push_str(indent);
            new_cursor = byte_to_char(&out, out.len());
            out.push_str(rest);
            out.push_str(&text[line.end..]);
        }
    }
    Some((out, new_cursor))
}

/// 한글 조합 중 Enter 처럼 TextEdit 이 이미 토글 제목 안에 줄바꿈을 넣었으면 (`<summary>제목` ↵ `</summary>`)
/// 그 줄바꿈을 되돌리고 [`enter_in_summary`] 처럼 토글 안으로 내려갑니다.
pub fn repair_split_summary(text: &str, cursor: usize) -> Option<(String, usize)> {
    let byte = char_to_byte(text, cursor);
    if byte == 0 || text.as_bytes()[byte - 1] != b'\n' {
        return None;
    }
    let prev = line_at(text, byte - 1);
    let p = text[prev].trim_start();
    if !p.starts_with("<summary>") || p.contains("</summary>") {
        return None;
    }
    let cur = line_at(text, byte);
    if !text[byte..cur.end].trim_end().ends_with("</summary>") {
        return None;
    }
    let mut joined = text.to_owned();
    joined.remove(byte - 1);
    enter_in_summary(&joined, cursor - 1)
}

/// `details_line` 바이트에서 시작하는 `<details …>` 줄의 open 을 바꿉니다.
pub fn set_toggle_open(text: &mut String, details_line: usize, open: bool) -> bool {
    if details_line > text.len() {
        return false;
    }
    let line = line_at(text, details_line);
    let raw = &text[line.clone()];
    if toggle_open_line(raw) != Some(!open) {
        return false;
    }
    let indent = raw[..raw.len() - raw.trim_start().len()].to_owned();
    let new = if open { format!("{indent}<details open>") } else { format!("{indent}<details>") };
    text.replace_range(line, &new);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, cursor: usize) -> Option<(String, usize)> {
        enter_in_list(text, cursor)
    }

    #[test]
    fn enter_in_toggle_title_goes_inside() {
        let t = "<details open>\n<summary>회의 메모</summary>\n\n\n\n</details>";
        let title_end = "<details open>\n<summary>회의 메모".chars().count();
        // 새 토글: 제목 끝에서 Enter 면 글은 그대로, 커서만 내용 첫 줄로
        let (out, c) = enter_in_summary(t, title_end).unwrap();
        assert_eq!(out, t);
        assert_eq!(out.chars().take(c).collect::<String>(), "<details open>\n<summary>회의 메모</summary>\n\n");
        // 제목 가운데에서 Enter 면 뒷부분이 내용 첫 줄로
        let (out, c) = enter_in_summary(t, title_end - 2).unwrap();
        assert_eq!(out, "<details open>\n<summary>회의 </summary>\n\n메모\n\n</details>");
        assert_eq!(out.chars().skip(c).take(2).collect::<String>(), "메모");
        // 접힌 토글은 펴고, 내용이 있으면 그 앞에 새 줄
        let t = "<details>\n  <summary>A</summary>\n\n  내용\n\n</details>";
        let (out, c) = enter_in_summary(t, "<details>\n  <summary>A".chars().count()).unwrap();
        assert_eq!(out, "<details open>\n  <summary>A</summary>\n\n  \n  내용\n\n</details>");
        assert_eq!(out.chars().take(c).collect::<String>(), "<details open>\n  <summary>A</summary>\n\n  ");
        // 제목 줄이 아니면 그대로
        assert!(enter_in_summary("<summary>A</summary>", 10).is_none(), "<details> 없이는 토글 아님");
        assert!(enter_in_summary(t, 2).is_none());
    }

    #[test]
    fn split_title_is_repaired() {
        // 한글 조합 중 Enter 로 제목 안에 줄바꿈이 들어간 직후
        let t = "<details open>\n<summary>ㅇㅇ\n</summary>\n\n\n\n</details>";
        let c = "<details open>\n<summary>ㅇㅇ\n".chars().count();
        let (out, c) = repair_split_summary(t, c).unwrap();
        assert_eq!(out, "<details open>\n<summary>ㅇㅇ</summary>\n\n\n\n</details>");
        assert_eq!(out.chars().take(c).collect::<String>(), "<details open>\n<summary>ㅇㅇ</summary>\n\n");
        // 보통 줄바꿈은 건드리지 않음
        assert!(repair_split_summary("글\n다음", 2).is_none());
        assert!(repair_split_summary("<summary>a</summary>\nb", 21).is_none());
    }

    #[test]
    fn continues_lists() {
        assert_eq!(run("- 사과", 4), Some(("- 사과\n- ".into(), 7)));
        assert_eq!(run("  * 배", 5), Some(("  * 배\n  * ".into(), 10)));
        assert_eq!(run("- [x] 끝", 7), Some(("- [x] 끝\n- [ ] ".into(), 14)));
        assert_eq!(run("9. 아홉", 5), Some(("9. 아홉\n10. ".into(), 10)));
        assert_eq!(run("1) 하나", 5), Some(("1) 하나\n2) ".into(), 9)));
        assert_eq!(run("> 인용", 4), Some(("> 인용\n> ".into(), 7)));
        // 가운데에서 Enter 하면 뒷부분이 새 항목으로
        assert_eq!(run("- 사과배", 4), Some(("- 사과\n- 배".into(), 7)));
    }

    #[test]
    fn empty_item_ends_or_outdents() {
        assert_eq!(run("- 사과\n- ", 7), Some(("- 사과\n".into(), 5)));
        assert_eq!(run("- 사과\n  - ", 9), Some(("- 사과\n- ".into(), 7)));
        assert_eq!(run("- [ ] ", 6), Some(("".into(), 0)));
    }

    #[test]
    fn plain_lines_are_left_alone() {
        assert_eq!(run("그냥 글", 3), None);
        assert_eq!(run("-이건 목록 아님", 3), None);
        assert_eq!(run("- 사과", 1), None, "기호 안쪽의 커서");
        assert_eq!(run("10/15 일정", 5), None);
    }

    #[test]
    fn tab_indents_list_lines() {
        let (t, r) = indent_list("- a\n- b", 1..1, false).unwrap();
        assert_eq!((t.as_str(), r), ("  - a\n- b", 3..3));
        let (t, r) = indent_list("- a\n  - b", 0..9, true).unwrap();
        assert_eq!((t.as_str(), r), ("- a\n- b", 0..7));
        assert_eq!(indent_list("글자", 0..0, false), None);
    }

    #[test]
    fn toggles() {
        let (t, c) = make_toggle("앞\n>> 회의 메모\n뒤", 9);
        assert_eq!(t, "앞\n<details open>\n<summary>회의 메모</summary>\n\n\n\n</details>\n뒤");
        assert_eq!(t.chars().take(c).collect::<String>(), "앞\n<details open>\n<summary>회의 메모");
        assert_eq!(toggle_open_line("<details open>"), Some(true));
        assert_eq!(toggle_open_line("  <details>"), Some(false));
        assert_eq!(toggle_open_line("<detailsx>"), None);
        assert_eq!(summary_line("<summary>제목</summary>"), Some(9..15));
        let at = toggle_at_cursor(&t, 20).unwrap();
        let mut t2 = t.clone();
        assert!(set_toggle_open(&mut t2, at, false));
        assert!(t2.starts_with("앞\n<details>\n"));
        assert!(!set_toggle_open(&mut t2, at, false), "이미 접힘");
    }
}
