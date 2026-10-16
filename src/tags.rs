//! 메모 태그: 글 속의 `#태그` 를 찾고, 사이드바 검색의 태그 검색식(AND · OR · 빼기)을 풉니다.
//!
//! - 태그는 줄 맨 앞이나 띄어쓰기 뒤의 `#` 에 글자 · 숫자 · `_` · `-` · `/` 가 이어진 것 (`#회의`, `#일/2024`)
//!   숫자로만 된 것(`#1`), 제목 기호(`# 제목`), 코드 안의 것은 태그가 아닙니다.
//! - `/` 로 나눈 하위 태그: `#일` 로 찾으면 `#일/회의` 도 찾습니다.
//! - 대소문자는 가리지 않습니다 (모두 소문자로 다룸).

use std::collections::BTreeSet;

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '/'
}

/// `s` 가 `#태그` 로 시작하면 (`#` 를 뺀 태그, `#` 를 포함한 바이트 길이).
/// 앞 글자가 띄어쓰기인지는 부르는 쪽에서 봅니다.
pub fn tag_at(s: &str) -> Option<(&str, usize)> {
    let rest = s.strip_prefix('#')?;
    let len = rest.find(|c: char| !is_tag_char(c)).unwrap_or(rest.len());
    let tag = rest[..len].trim_end_matches(['/', '-']);
    if tag.is_empty() || tag.chars().all(|c| c.is_ascii_digit()) || tag.starts_with('/') {
        return None;
    }
    Some((tag, 1 + tag.len()))
}

/// 글 속의 태그들 (소문자, 겹치지 않게, 이름 순). 코드 블록 · `코드` 안은 뺍니다.
pub fn extract(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if let Some(f) = ["```", "~~~"].into_iter().find(|f| t.starts_with(f)) {
            fence = Some(f);
            continue;
        }
        let mut in_code = false;
        let mut prev: Option<char> = None;
        for (i, c) in line.char_indices() {
            if c == '`' {
                in_code = !in_code;
            } else if c == '#'
                && !in_code
                && prev.is_none_or(char::is_whitespace)
                && let Some((tag, _)) = tag_at(&line[i..])
            {
                out.insert(tag.to_lowercase());
            }
            prev = Some(c);
        }
    }
    out
}

/// 태그 검색식: OR 로 나뉜 묶음들, 묶음 안은 모두 맞아야(AND) 함.
/// `#a #b` (둘 다), `#a OR #b` · `#a | #b` (둘 중 하나), `-#a` · `NOT #a` (빼기).
/// `AND` · `&` 는 써도 되고 안 써도 됩니다. 한국어로 `그리고` · `또는` · `빼고` 도 됩니다.
#[derive(Debug, Default, PartialEq)]
pub struct TagQuery {
    /// (있어야 할 태그, 없어야 할 태그) 묶음들
    pub groups: Vec<(Vec<String>, Vec<String>)>,
    /// 마지막으로 치고 있는 태그 (태그 추천용, `#` 만 쳤으면 빈 글자)
    pub typing: Option<String>,
}

impl TagQuery {
    /// 태그와 연산자로만 된 검색어면 태그 검색식. 다른 낱말이 섞이면 `None` (보통 검색)
    pub fn parse(query: &str) -> Option<TagQuery> {
        let spaced = query.replace('|', " | ").replace('&', " & ").replace(['(', ')'], " ");
        let tokens: Vec<&str> = spaced.split_whitespace().collect();
        if !tokens.iter().any(|t| t.trim_start_matches('-').starts_with('#')) {
            return None;
        }
        let mut q = TagQuery { groups: vec![(Vec::new(), Vec::new())], typing: None };
        let mut negate = false;
        for (n, tok) in tokens.iter().enumerate() {
            let last = n + 1 == tokens.len();
            let lower = tok.to_lowercase();
            match lower.as_str() {
                "or" | "|" | "또는" => {
                    q.groups.push((Vec::new(), Vec::new()));
                    negate = false;
                    continue;
                }
                "and" | "&" | "그리고" => continue,
                "not" | "빼고" => {
                    negate = true;
                    continue;
                }
                _ => {}
            }
            let (neg, body) = match lower.strip_prefix('-') {
                Some(b) => (true, b),
                None => (negate, lower.as_str()),
            };
            negate = false;
            if body == "#" {
                if last {
                    q.typing = Some(String::new());
                    continue;
                }
                return None;
            }
            let (tag, len) = tag_at(body)?;
            if len != body.len() {
                return None;
            }
            if last {
                q.typing = Some(tag.to_owned());
            }
            let g = q.groups.last_mut()?;
            if neg { g.1.push(tag.to_owned()) } else { g.0.push(tag.to_owned()) }
        }
        q.groups.retain(|(inc, exc)| !inc.is_empty() || !exc.is_empty());
        Some(q)
    }

    /// 이 태그들을 가진 메모가 검색식에 맞는지 (검색식이 비었으면 아무것도 맞지 않음)
    pub fn matches(&self, tags: &BTreeSet<String>) -> bool {
        let has = |want: &String| {
            tags.iter().any(|t| t == want || t.strip_prefix(want.as_str()).is_some_and(|r| r.starts_with('/')))
        };
        self.groups.iter().any(|(inc, exc)| {
            // 빼기만 있는 묶음은 "태그가 하나라도 있는 메모 중에서" 뺌
            (!inc.is_empty() || !tags.is_empty()) && inc.iter().all(has) && !exc.iter().any(has)
        })
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// 검색어의 마지막 태그(치는 중인 것)를 `tag` 로 바꾸고 이어서 칠 수 있게 띄어 씁니다.
/// 마지막 낱말이 태그가 아니면 (`OR` 등) 뒤에 붙입니다.
pub fn complete_last(query: &str, tag: &str) -> String {
    let trimmed = query.trim_end();
    let last_start = trimmed.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let last = &trimmed[last_start..];
    let prefix = if last.starts_with("-#") {
        "-#"
    } else if last.starts_with('#') {
        "#"
    } else {
        return format!("{trimmed} #{tag} ").trim_start().to_owned();
    };
    format!("{}{prefix}{tag} ", &trimmed[..last_start])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(tags: &[&str]) -> BTreeSet<String> {
        tags.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn finds_tags_in_text() {
        let text = "# 제목\n오늘 #회의 #일/2024 그리고 #TODO.\n#1 은 번호, C#과 a#b, [링크](#위로) 는 아님\n\
                    `#코드` 와\n```\n#블록\n```\n- #목록 끝 #a-";
        assert_eq!(extract(text), set(&["a", "todo", "목록", "일/2024", "회의"]));
        assert_eq!(tag_at("#회의,"), Some(("회의", 7)));
        assert_eq!(tag_at("# 제목"), None);
        assert_eq!(tag_at("#2024"), None);
    }

    #[test]
    fn parses_and_or_not() {
        let q = TagQuery::parse("#회의 #예산").unwrap();
        assert_eq!(q.groups, vec![(vec!["회의".to_string(), "예산".to_string()], vec![])]);
        let q = TagQuery::parse("#회의 OR #일").unwrap();
        assert_eq!(q.groups.len(), 2);
        assert_eq!(TagQuery::parse("#a|#b").unwrap().groups.len(), 2);
        assert_eq!(TagQuery::parse("#a and #b").unwrap().groups[0].0.len(), 2);
        assert_eq!(TagQuery::parse("#a 또는 #b").unwrap().groups.len(), 2);
        let q = TagQuery::parse("#a -#b").unwrap();
        assert_eq!(q.groups, vec![(vec!["a".to_string()], vec!["b".to_string()])]);
        assert_eq!(TagQuery::parse("#a not #b").unwrap(), q);
        // 보통 낱말이 섞이면 태그 검색이 아님
        assert!(TagQuery::parse("회의록").is_none());
        assert!(TagQuery::parse("#회의 예산").is_none());
        // 치는 중
        assert_eq!(TagQuery::parse("#").unwrap().typing.as_deref(), Some(""));
        assert_eq!(TagQuery::parse("#a #회").unwrap().typing.as_deref(), Some("회"));
        assert!(TagQuery::parse("#").unwrap().is_empty());
    }

    #[test]
    fn completes_the_last_tag() {
        assert_eq!(complete_last("#", "회의"), "#회의 ");
        assert_eq!(complete_last("#a #회", "회의"), "#a #회의 ");
        assert_eq!(complete_last("#a OR", "b"), "#a OR #b ");
        assert_eq!(complete_last("#a -#b", "bc"), "#a -#bc ");
    }

    #[test]
    fn matches_notes() {
        let note = set(&["회의", "일/2024", "예산"]);
        let m = |q: &str| TagQuery::parse(q).unwrap().matches(&note);
        assert!(m("#회의"));
        assert!(m("#회의 #예산"));
        assert!(!m("#회의 #여행"));
        assert!(m("#여행 OR #예산"));
        assert!(!m("#여행 | #취미"));
        assert!(m("#일"), "하위 태그도");
        assert!(!m("#일/2023"));
        assert!(!m("#회의 -#예산"));
        assert!(m("#회의 -#여행"));
        assert!(m("#여행 OR #회의 -#여행"), "OR 묶음마다 따로");
        assert!(m("-#여행"), "빼기만 있으면 태그 있는 메모 중에서");
        assert!(!TagQuery::parse("-#여행").unwrap().matches(&set(&[])));
        assert!(!m("#"), "빈 검색식");
    }
}
