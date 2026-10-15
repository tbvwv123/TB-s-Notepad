//! 메모 사이 링크: 다른 메모 · 폴더 · 줄 · 제목을 가리키는 링크를 풀고 만듭니다.
//!
//! - `[회의록](회의/10월.md)` → 그 메모를 앱에서 엽니다
//! - `[3번째 줄](회의/10월.md#L3)` → 그 메모의 3번째 줄로
//! - `[결정 사항](회의/10월.md#결정-사항)` → 그 메모의 `## 결정 사항` 제목으로
//! - `[자료](../자료)` → 메모 폴더 안이면 사이드바에서 그 폴더를 펼쳐 보여 줌
//! - `[위로](#L1)` · `[목차](#목차)` → 지금 메모 안에서 이동

use std::path::{Component, Path, PathBuf};

/// 메모 안에서 갈 곳
#[derive(Clone, Debug, PartialEq)]
pub enum Anchor {
    /// 1부터 세는 줄 번호
    Line(usize),
    /// 제목 글자 (비교할 때는 [`slug`]로 맞춤)
    Heading(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// `None` 이면 지금 메모
    pub path: Option<PathBuf>,
    pub anchor: Option<Anchor>,
}

fn parse_anchor(fragment: &str) -> Option<Anchor> {
    let f = crate::markdown::percent_decode(fragment.trim());
    if f.is_empty() {
        return None;
    }
    if let Some(n) = f.strip_prefix(['L', 'l']).and_then(|n| n.split('-').next()).and_then(|n| n.parse().ok())
        && n > 0
    {
        return Some(Anchor::Line(n));
    }
    Some(Anchor::Heading(f))
}

/// 웹 주소가 아닌 링크를 풉니다. 상대 경로는 `base_dir`(지금 메모의 폴더) 기준.
pub fn parse(url: &str, base_dir: Option<&Path>) -> Option<Target> {
    let url = url.trim().trim_start_matches('<').trim_end_matches('>').trim();
    if url.is_empty() {
        return None;
    }
    if let Some(fragment) = url.strip_prefix('#') {
        return Some(Target { path: None, anchor: Some(parse_anchor(fragment)?) });
    }
    // 이름에 # 이 들어간 파일도 있으니, 통째로 있는 파일이면 그대로
    if let Some(p) = crate::media::local_link_target(url, base_dir)
        && p.exists()
    {
        return Some(Target { path: Some(p), anchor: None });
    }
    let (path_part, fragment) = match url.rsplit_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (url, None),
    };
    let path = crate::media::local_link_target(path_part, base_dir)?;
    Some(Target { path: Some(path), anchor: fragment.and_then(parse_anchor) })
}

/// 제목 비교용: 소문자, 기호 빼기, 띄어쓰기는 `-` (GitHub 방식과 비슷)
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for ch in s.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if (ch == '-' || ch.is_whitespace()) && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

/// `# 제목` 줄이면 제목 글자
pub fn heading_text(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let hashes = t.bytes().take_while(|&c| c == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim())
}

/// 갈 곳의 글자 범위 (그 줄 전체, TextEdit 커서 단위)
pub fn anchor_range(text: &str, anchor: &Anchor) -> Option<std::ops::Range<usize>> {
    let want = match anchor {
        Anchor::Heading(h) => Some(slug(h)),
        Anchor::Line(_) => None,
    };
    let mut in_code = false;
    let mut chars = 0;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let body = line.trim_end_matches(['\n', '\r']);
        let len = body.chars().count();
        let hit = match (anchor, &want) {
            (Anchor::Line(n), _) => i + 1 == *n,
            (_, Some(want)) => {
                let t = body.trim_start();
                if t.starts_with("```") || t.starts_with("~~~") {
                    in_code = !in_code;
                }
                !in_code && heading_text(body).is_some_and(|h| slug(h) == *want)
            }
            _ => false,
        };
        if hit {
            return Some(chars..chars + len);
        }
        chars += line.chars().count();
    }
    // 파일 끝이 줄바꿈이면 마지막 빈 줄도 셈
    if let Anchor::Line(n) = anchor
        && *n == text.split_inclusive('\n').count() + 1
        && text.ends_with('\n')
    {
        return Some(chars..chars);
    }
    None
}

fn same_component(a: &Component<'_>, b: &Component<'_>) -> bool {
    if cfg!(windows) {
        a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// `from_dir` 에서 `to` 로 가는 상대 경로 (드라이브가 다르면 `None`)
pub fn relative_path(from_dir: &Path, to: &Path) -> Option<PathBuf> {
    let a: Vec<_> = from_dir.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| same_component(x, y)).count();
    if common == 0 {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..a.len() {
        out.push("..");
    }
    for c in &b[common..] {
        out.push(c);
    }
    Some(out)
}

/// 링크 주소로 쓸 글자: `/` 로 나누고, 빈칸 등이 있으면 `<...>`
pub fn link_dest(path: &Path, anchor: Option<&str>) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        s = ".".into();
    }
    if let Some(a) = anchor {
        s.push('#');
        s.push_str(a);
    }
    if s.contains([' ', '(', ')', '<', '>']) { format!("<{s}>") } else { s }
}

/// 글 속 `[..](주소)` · `![..](주소)` 의 주소마다 `f` 를 불러, `Some(새 주소)` 면 바꿉니다.
/// 하나도 안 바뀌면 `None`. 주소 뒤의 제목(`"width=50%"`)은 그대로 둡니다.
fn map_link_dests(text: &str, mut f: impl FnMut(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        out.push_str(&rest[..i + 2]);
        rest = &rest[i + 2..];
        let (raw, after) = if rest.starts_with('<') {
            match rest.find('>') {
                Some(e) => (&rest[..=e], &rest[e + 1..]),
                None => break,
            }
        } else {
            let e = rest.find([')', ' ', '\n']).unwrap_or(rest.len());
            (&rest[..e], &rest[e..])
        };
        match f(raw) {
            Some(r) if r != raw => {
                out.push_str(&r);
                changed = true;
            }
            _ => out.push_str(raw),
        }
        rest = after;
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// 주소 원문에서 `#` 뒤의 위치 (줄·제목)
fn anchor_of(raw: &str) -> Option<&str> {
    raw.trim_start_matches('<').trim_end_matches('>').rsplit_once('#').map(|(_, a)| a)
}

/// 붙여넣는 글 속 `[..](절대 경로)` 링크를, 같은 메모 폴더 안이면 지금 메모 기준 상대 경로로 바꿉니다.
/// ("이 줄 링크 복사" · "링크 복사"는 어디에 붙일지 몰라 절대 경로로 복사합니다.)
pub fn relativize_links(text: &str, doc_dir: &Path, roots: &[PathBuf]) -> Option<String> {
    let root_of = |p: &Path| roots.iter().find(|r| p.starts_with(r)).cloned();
    let doc_root = root_of(doc_dir)?;
    map_link_dests(text, |raw| {
        let t = parse(raw, None)?;
        let path = t.path?;
        if root_of(&path).as_ref() != Some(&doc_root) {
            return None;
        }
        let rel = relative_path(doc_dir, &path)?;
        Some(link_dest(&rel, anchor_of(raw).filter(|_| t.anchor.is_some())))
    })
}

/// `..` 과 `.` 을 정리한 경로 (디스크를 보지 않음)
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// 파일·폴더를 `moved_from` 에서 `moved_to` 로 옮긴 뒤, 한 메모(`old_dir` 에 있다가 지금은 `new_dir`)의 링크를 고칩니다.
/// 옮긴 것을 가리키던 링크와, 메모 자신이 옮겨져서 상대 경로가 달라진 링크(그림 포함)를 새 위치로 바꿉니다.
/// 웹 주소 · `#` 만 있는 링크 · 지금도 없는 파일을 가리키는 링크는 그대로 둡니다.
pub fn rebase_links(text: &str, old_dir: &Path, new_dir: &Path, moved_from: &Path, moved_to: &Path) -> Option<String> {
    map_link_dests(text, |raw| {
        let inner = raw.trim_start_matches('<').trim_end_matches('>');
        if inner.starts_with('#') || inner.contains("://") || inner.starts_with("mailto:") || inner.starts_with("data:")
        {
            return None;
        }
        let t = parse(raw, Some(old_dir))?;
        let old_target = normalize(&t.path?);
        let new_target = match old_target.strip_prefix(moved_from) {
            Ok(rest) if rest.as_os_str().is_empty() => moved_to.to_path_buf(),
            Ok(rest) => moved_to.join(rest),
            Err(_) => old_target.clone(),
        };
        if new_target == old_target && new_dir == old_dir {
            return None;
        }
        if !new_target.exists() {
            return None;
        }
        let anchor = anchor_of(raw).filter(|_| t.anchor.is_some());
        // 원래 절대 경로로 썼으면 절대 경로로
        let was_absolute = crate::media::local_link_target(inner.split('#').next().unwrap_or(inner), None).is_some();
        if was_absolute {
            return Some(link_dest(&new_target, anchor));
        }
        Some(link_dest(&relative_path(new_dir, &new_target)?, anchor))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_note_links() {
        let base = Path::new("C:/notes/일기");
        assert_eq!(parse("#L12", Some(base)), Some(Target { path: None, anchor: Some(Anchor::Line(12)) }));
        assert_eq!(
            parse("#처음-할-일", Some(base)),
            Some(Target { path: None, anchor: Some(Anchor::Heading("처음-할-일".into())) })
        );
        let t = parse("../회의/10월.md#L3", Some(base)).unwrap();
        assert!(t.path.unwrap().ends_with(Path::new("회의").join("10월.md")));
        assert_eq!(t.anchor, Some(Anchor::Line(3)));
        let t = parse("<C:/notes/회의 기록.md#결정%20사항>", None).unwrap();
        assert_eq!(t.path, Some(PathBuf::from("C:\\notes\\회의 기록.md")));
        assert_eq!(t.anchor, Some(Anchor::Heading("결정 사항".into())));
        assert_eq!(parse("https://example.com#a", Some(base)), None);
    }

    #[test]
    fn finds_lines_and_headings() {
        let text = "# 제목\n본문\n```\n# 코드 속 주석\n```\n## 결정 사항!\n끝";
        assert_eq!(anchor_range(text, &Anchor::Line(2)), Some(5..7));
        assert_eq!(anchor_range(text, &Anchor::Line(99)), None);
        let r = anchor_range(text, &Anchor::Heading("결정 사항".into())).unwrap();
        assert_eq!(text.chars().skip(r.start).take(r.len()).collect::<String>(), "## 결정 사항!");
        assert_eq!(anchor_range(text, &Anchor::Heading("결정-사항".into())), Some(r));
        assert_eq!(anchor_range(text, &Anchor::Heading("코드 속 주석".into())), None);
        assert_eq!(slug("  Hello, World — 안녕! "), "hello-world-안녕");
    }

    #[test]
    fn relative_paths() {
        let r = relative_path(Path::new("C:/n/a/b"), Path::new("C:/n/c/d.md")).unwrap();
        assert_eq!(r, Path::new("..").join("..").join("c").join("d.md"));
        assert_eq!(relative_path(Path::new("C:/n"), Path::new("C:/n/x.md")).unwrap(), Path::new("x.md"));
        assert_eq!(relative_path(Path::new("C:/n"), Path::new("D:/x.md")), None);
    }

    #[test]
    fn links_follow_moves() {
        let base = std::env::temp_dir().join(format!("tbn_rebase_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["notes/a/images", "notes/b", "notes/c"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("notes/a/images/p.png"), b"x").unwrap();
        std::fs::write(base.join("notes/b/target.md"), b"x").unwrap();
        let (a, b, c) = (base.join("notes/a"), base.join("notes/b"), base.join("notes/c"));

        // 메모가 a → c 로 옮겨짐: 그림과 다른 메모 링크가 새 위치 기준으로
        let note = "![p](images/p.png \"width=50%\") [t](../b/target.md#L3) [web](https://x.y) [없음](nope.md)";
        let out = rebase_links(note, &a, &c, &a.join("n.md"), &c.join("n.md")).unwrap();
        assert_eq!(
            out,
            "![p](../a/images/p.png \"width=50%\") [t](../b/target.md#L3) [web](https://x.y) [없음](nope.md)"
        );

        // b/target.md → c/target.md 로 옮김: 그걸 가리키던 다른 메모의 링크가 따라감
        std::fs::rename(b.join("target.md"), c.join("target.md")).unwrap();
        let other = "[t](../b/target.md#결정) [p](images/p.png)";
        let out = rebase_links(other, &a, &a, &b.join("target.md"), &c.join("target.md")).unwrap();
        assert_eq!(out, "[t](../c/target.md#결정) [p](images/p.png)");
        // 상관없는 메모는 그대로
        assert_eq!(rebase_links("[p](images/p.png)", &a, &a, &b, &c), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pasted_links_become_relative() {
        let roots = vec![PathBuf::from("C:/notes")];
        let doc = Path::new("C:/notes/일기");
        let text = "보기: [회의 10줄](<C:/notes/회의 기록/10월.md#L10>) 와 [밖](C:/other/x.md) 와 [웹](https://a.b)";
        let out = relativize_links(text, doc, &roots).unwrap();
        assert_eq!(out, "보기: [회의 10줄](<../회의 기록/10월.md#L10>) 와 [밖](C:/other/x.md) 와 [웹](https://a.b)");
        assert_eq!(relativize_links("[웹](https://a.b)", doc, &roots), None);
        assert_eq!(relativize_links("그냥 글", doc, &roots), None);
    }
}
