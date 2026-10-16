//! 기본 이모지: 자주 쓰는 약 300개의 트위모지 그림 (실행 파일에 들어 있음, 인터넷 연결 없음).
//!
//! 글에는 보통 유니코드 이모지 글자(😄)로 넣으므로 다른 앱에서도 그대로 보이고,
//! 문서 모드에서는 그 글자 자리에 트위모지 그림을 그립니다. 목록은 `scripts/twemoji_list.py`.
//! 트위모지 그림: jdecked/twemoji, CC-BY 4.0 (`assets/twemoji/NOTICE.md`).

use std::collections::HashMap;
use std::sync::OnceLock;

pub use crate::twemoji_table::TWEMOJI;

const VS16: char = '\u{fe0f}';

/// 그림 주소 (`install` 로 egui 에 넣어 둔 바이트)
pub fn uri(i: usize) -> String {
    format!("bytes://twemoji/{i}.png")
}

/// 앱을 켤 때 한 번: 그림 바이트를 egui 에 등록
pub fn install(ctx: &egui::Context) {
    for (i, (_, _, bytes)) in TWEMOJI.iter().enumerate() {
        ctx.include_bytes(uri(i), *bytes);
    }
}

fn by_char() -> &'static HashMap<char, usize> {
    static MAP: OnceLock<HashMap<char, usize>> = OnceLock::new();
    MAP.get_or_init(|| TWEMOJI.iter().enumerate().filter_map(|(i, (_, s, _))| Some((s.chars().next()?, i))).collect())
}

/// `s` 가 기본 이모지 글자로 시작하면 (번호, 바이트 길이 — 뒤의 VS16 포함)
pub fn at(s: &str) -> Option<(usize, usize)> {
    let c = s.chars().next()?;
    let &i = by_char().get(&c)?;
    let mut len = c.len_utf8();
    if s[len..].starts_with(VS16) {
        len += VS16.len_utf8();
    }
    Some((i, len))
}

/// 이름이 `query` 로 시작하는 것부터, 그다음 들어 있는 것
pub fn filter(query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    let mut hits: Vec<(u8, usize)> = TWEMOJI
        .iter()
        .enumerate()
        .filter_map(|(i, (name, _, _))| {
            if name.starts_with(&q) {
                Some((0, i))
            } else if name.contains(&q) {
                Some((1, i))
            } else {
                None
            }
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, i)| i).collect()
}

/// 이름이 꼭 맞는 것 (`:smile:` 을 끝까지 쳤을 때)
pub fn find(name: &str) -> Option<usize> {
    TWEMOJI.iter().position(|(n, _, _)| n.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sane() {
        assert!(TWEMOJI.len() >= 250);
        for (name, s, bytes) in TWEMOJI {
            assert!(bytes.starts_with(b"\x89PNG"), "{name}");
            assert!(at(s).is_some_and(|(_, len)| len == s.len()), "{name}");
        }
        let names: std::collections::HashSet<_> = TWEMOJI.iter().map(|(n, _, _)| n).collect();
        assert_eq!(names.len(), TWEMOJI.len(), "이름이 겹치지 않음");
    }

    #[test]
    fn finds_by_char_and_name() {
        let (i, len) = at("😄 다음").unwrap();
        assert_eq!((TWEMOJI[i].0, len), ("smile", 4));
        // ❤ 는 VS16 이 붙어 있어도, 없어도
        assert_eq!(at("❤️!").map(|(_, l)| l), Some(6));
        assert_eq!(at("❤!").map(|(_, l)| l), Some(3));
        assert!(at("가").is_none());
        assert_eq!(TWEMOJI[filter("thumb")[0]].0, "thumbsup");
        assert!(find("TADA").is_some());
    }
}
