//! 화면 언어: 한국어 · English · 中文(简体) · 日本語.
//!
//! 코드에는 한국어 문구를 그대로 쓰고 `t!("저장")` 처럼 감싸면, 지금 언어의 번역을 표(`i18n_table.rs`)에서 찾아 줍니다.
//! 번역이 없으면 한국어 그대로 나옵니다. 값이 들어가는 문구는 `tf!("{} 저장됨", name)` 처럼 `{}` 자리에 차례로 채웁니다.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    Ko,
    En,
    Zh,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::Ko, Lang::En, Lang::Zh, Lang::Ja];

    /// 설정 창에 보이는 이름 (그 언어로)
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Ko => "한국어",
            Lang::En => "English",
            Lang::Zh => "中文",
            Lang::Ja => "日本語",
        }
    }

    /// HTML `lang` 속성 등에 쓰는 코드
    pub fn code(self) -> &'static str {
        match self {
            Lang::Ko => "ko",
            Lang::En => "en",
            Lang::Zh => "zh-CN",
            Lang::Ja => "ja",
        }
    }

    /// Windows 화면 언어 (설정에서 "자동"일 때)
    pub fn system() -> Lang {
        #[cfg(windows)]
        {
            // SAFETY: 인자 없는 단순 조회
            let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
            match id & 0x3ff {
                0x12 => Lang::Ko,
                0x04 => Lang::Zh,
                0x11 => Lang::Ja,
                _ => Lang::En,
            }
        }
        #[cfg(not(windows))]
        {
            let l = std::env::var("LANG").unwrap_or_default().to_lowercase();
            if l.starts_with("ko") {
                Lang::Ko
            } else if l.starts_with("zh") {
                Lang::Zh
            } else if l.starts_with("ja") {
                Lang::Ja
            } else {
                Lang::En
            }
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        2 => Lang::Zh,
        3 => Lang::Ja,
        _ => Lang::Ko,
    }
}

/// (한국어, English, 中文, 日本語)
type Row = (&'static str, &'static str, &'static str, &'static str);

fn table() -> &'static HashMap<&'static str, [&'static str; 3]> {
    static MAP: OnceLock<HashMap<&'static str, [&'static str; 3]>> = OnceLock::new();
    MAP.get_or_init(|| crate::i18n_table::ROWS.iter().map(|&(ko, en, zh, ja): &Row| (ko, [en, zh, ja])).collect())
}

/// 한국어 문구의 지금 언어 번역
pub fn tr(ko: &'static str) -> &'static str {
    tr_in(lang(), ko)
}

pub fn tr_in(lang: Lang, ko: &'static str) -> &'static str {
    match lang {
        Lang::Ko => ko,
        other => table().get(ko).map(|row| row[other.index() - 1]).filter(|s| !s.is_empty()).unwrap_or(ko),
    }
}

/// `{}` 자리를 차례로 채웁니다 (`{{` `}}` 는 중괄호 글자)
pub fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' if chars.peek() == Some(&'}') => {
                chars.next();
                if let Some(a) = args.next() {
                    out.push_str(&a.to_string());
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// 번역된 문구 (`&'static str`)
#[macro_export]
macro_rules! t {
    ($ko:literal) => {
        $crate::i18n::tr($ko)
    };
}

/// 번역된 문구에 값 채우기 (`String`)
#[macro_export]
macro_rules! tf {
    ($ko:literal $(, $arg:expr)* $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($ko), &[$(&$arg as &dyn ::std::fmt::Display),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders() {
        assert_eq!(fill("{}줄, {}열", &[&3, &"7"]), "3줄, 7열");
        assert_eq!(fill("{{}} {}", &[&1]), "{} 1");
        assert_eq!(fill("모자람 {} {}", &[&1]), "모자람 1 ");
    }

    #[test]
    fn every_row_is_translated_once() {
        let mut seen = std::collections::HashSet::new();
        for (ko, en, zh, ja) in crate::i18n_table::ROWS {
            assert!(seen.insert(*ko), "중복: {ko}");
            assert!(!en.is_empty() && !zh.is_empty() && !ja.is_empty(), "번역 빠짐: {ko}");
            // 자리표시 개수가 같아야 값이 빠지지 않음
            let holes = |s: &str| s.matches("{}").count();
            assert_eq!(holes(ko), holes(en), "{ko} / {en}");
            assert_eq!(holes(ko), holes(zh), "{ko} / {zh}");
            assert_eq!(holes(ko), holes(ja), "{ko} / {ja}");
        }
    }

    #[test]
    fn lookup_falls_back_to_korean() {
        assert_eq!(tr_in(Lang::Ko, "저장"), "저장");
        assert_eq!(tr_in(Lang::En, "이런 문구는 표에 없음"), "이런 문구는 표에 없음");
    }
}
