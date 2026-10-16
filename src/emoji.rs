//! 커스텀 이모지: 메모 폴더의 `.emoji/` 에 둔 그림을 `:이름:` 으로 골라 글 속에 글자 크기로 넣습니다.
//!
//! 글에는 보통 그림 문법으로 저장하므로 다른 마크다운 앱에서도 그림으로 보입니다:
//! `![:이름:](../.emoji/이름.png "emoji")` — 제목 `"emoji"` 가 있으면 이 앱은 줄 안에 작게 그립니다.

use std::path::{Path, PathBuf};

/// 메모 폴더 안의 이모지 폴더 이름 (점으로 시작해서 사이드바에는 보이지 않음)
pub const DIR: &str = ".emoji";

/// 그림 문법의 제목: 이게 있으면 이모지
pub const TITLE: &str = "emoji";

/// 이모지로 쓸 수 있는 그림
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

#[derive(Clone, Debug, PartialEq)]
pub struct Emoji {
    pub name: String,
    pub path: PathBuf,
}

/// 메모 폴더(`root`)의 이모지 폴더
pub fn dir_for(root: &Path) -> PathBuf {
    root.join(DIR)
}

/// 이모지 폴더의 그림들 (이름 순). 이름이 같으면 앞의 것만.
pub fn list(dir: &Path) -> Vec<Emoji> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Emoji> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && has_image_extension(p))
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            (clean_name(&name) == name && !name.is_empty()).then_some(Emoji { name, path })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.path.cmp(&b.path)));
    out.dedup_by(|b, a| a.name.eq_ignore_ascii_case(&b.name));
    out
}

fn has_image_extension(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

/// 이모지 이름으로 쓸 수 있게: 글자 · 숫자 · `_` · `-` 만 (띄어쓰기는 `_`), 32자까지
pub fn clean_name(s: &str) -> String {
    let name: String = s
        .trim()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                Some(c)
            } else if c.is_whitespace() {
                Some('_')
            } else {
                None
            }
        })
        .take(32)
        .collect();
    name.trim_matches('_').to_owned()
}

/// `query` 에 맞는 이모지 번호: 이름이 그것으로 시작하는 것부터, 그다음 들어 있는 것
pub fn filter(list: &[Emoji], query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    let mut hits: Vec<(u8, usize)> = list
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let name = e.name.to_lowercase();
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

/// 그림 `source` 를 이모지 폴더에 `name` 으로 복사합니다. 같은 이름이 있으면 `AlreadyExists`.
pub fn register(dir: &Path, source: &Path, name: &str) -> std::io::Result<Emoji> {
    if !has_image_extension(source) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not an image"));
    }
    if list(dir).iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    std::fs::create_dir_all(dir)?;
    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let path = dir.join(format!("{name}.{ext}"));
    std::fs::copy(source, &path)?;
    Ok(Emoji { name: name.to_owned(), path })
}

/// 글에 넣을 문법 (`dest` 는 링크 주소 그대로)
pub fn markdown(name: &str, dest: &str) -> String {
    format!("![:{name}:]({dest} \"{TITLE}\")")
}

/// 글 속의 이모지 하나
#[derive(Debug, PartialEq)]
pub struct Inline<'a> {
    /// 문법 전체의 바이트 길이
    pub len: usize,
    pub name: &'a str,
    /// 그림 주소 (`<...>` 는 벗긴 것)
    pub dest: &'a str,
}

/// `s` 가 이모지 문법으로 시작하면 그 이모지
pub fn parse_inline(s: &str) -> Option<Inline<'_>> {
    let rest = s.strip_prefix("![:")?;
    let name_end = rest.find(":](")?;
    let name = &rest[..name_end];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    let after = &rest[name_end + 3..];
    let (dest, after) = match after.strip_prefix('<') {
        Some(r) => {
            let e = r.find(['>', '\n'])?;
            (&r[..e], r[e..].strip_prefix('>')?)
        }
        None => {
            let e = after.find([' ', '\n', ')'])?;
            (&after[..e], &after[e..])
        }
    };
    let after = after.strip_prefix(" \"")?.strip_prefix(TITLE)?.strip_prefix("\")")?;
    (!dest.is_empty()).then_some(Inline { len: s.len() - after.len(), name, dest })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name(" party parrot! "), "party_parrot");
        assert_eq!(clean_name("고양이-웃음"), "고양이-웃음");
        assert_eq!(clean_name("a:b/c"), "abc");
        assert_eq!(clean_name("!!!"), "");
    }

    #[test]
    fn parses_inline_emoji() {
        let s = "![:party:](../.emoji/party.png \"emoji\") 다음 글";
        let e = parse_inline(s).unwrap();
        assert_eq!((e.name, e.dest), ("party", "../.emoji/party.png"));
        assert_eq!(&s[e.len..], " 다음 글");
        let s = markdown("웃음", "<my emoji/웃음.png>");
        assert_eq!(parse_inline(&s).map(|e| (e.dest, e.len)), Some(("my emoji/웃음.png", s.len())));
        // 이모지가 아닌 그림 · 링크
        assert!(parse_inline("![:party:](party.png)").is_none());
        assert!(parse_inline("![party](party.png \"emoji\")").is_none());
        assert!(parse_inline("![:a b:](a.png \"emoji\")").is_none());
        assert!(parse_inline("![:a:](a.png \"width=50%\")").is_none());
    }

    #[test]
    fn filters_by_prefix_then_substring() {
        let list: Vec<Emoji> = ["cat", "happy_cat", "catjam", "dog"]
            .into_iter()
            .map(|n| Emoji { name: n.into(), path: PathBuf::from(format!("{n}.png")) })
            .collect();
        let names = |q| filter(&list, q).into_iter().map(|i| list[i].name.as_str()).collect::<Vec<_>>();
        assert_eq!(names("cat"), ["cat", "catjam", "happy_cat"]);
        assert_eq!(names("CAT")[0], "cat");
        assert_eq!(names("x"), Vec::<&str>::new());
        assert_eq!(names("").len(), 4);
    }

    #[test]
    fn registers_and_lists() {
        let base = std::env::temp_dir().join(format!("tbn_emoji_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let src = base.join("Party Parrot.PNG");
        std::fs::write(&src, b"png").unwrap();
        let dir = dir_for(&base);
        let e = register(&dir, &src, "parrot").unwrap();
        assert_eq!(e.path, dir.join("parrot.png"));
        assert_eq!(
            register(&dir, &src, "Parrot").unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists,
            "대소문자만 다른 이름도 겹침"
        );
        assert!(register(&dir, &base.join("note.md"), "x").is_err(), "그림만");
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join("bad name.png"), b"x").unwrap();
        std::fs::write(dir.join("cat.gif"), b"x").unwrap();
        let names: Vec<String> = list(&dir).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["cat", "parrot"], "그림이 아니거나 이름이 이모지 이름이 아니면 뺌");
        let _ = std::fs::remove_dir_all(&base);
    }
}
