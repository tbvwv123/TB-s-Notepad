//! 마크다운 관련 기능: 편집기 문법 강조, 미리보기 준비, HTML 내보내기, 서식 도구.

use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::Path;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

use crate::settings::{Palette, c, mix};

// ---------------------------------------------------------------------------
// 문법 강조

#[derive(Clone)]
pub struct HighlightStyle {
    pub font: FontId,
    pub line_spacing: f32,
    pub markdown: bool,
    pub text: Color32,
    pub muted: Color32,
    pub heading: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub link: Color32,
    pub quote: Color32,
    pub accent: Color32,
}

impl HighlightStyle {
    pub fn new(palette: &Palette, font: FontId, line_spacing: f32, markdown: bool) -> Self {
        let bg = c(palette.editor_bg);
        let text = c(palette.text);
        Self {
            font,
            line_spacing,
            markdown,
            text,
            muted: c(palette.muted),
            heading: c(palette.heading),
            code: c(palette.code),
            code_bg: mix(bg, text, 0.07),
            link: c(palette.link),
            quote: c(palette.quote),
            accent: c(palette.accent),
        }
    }

    /// 캐시 키: 텍스트와 스타일이 같으면 같은 값
    pub fn cache_key(&self, text: &str) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        self.font.size.to_bits().hash(&mut h);
        self.font.family.hash(&mut h);
        self.line_spacing.to_bits().hash(&mut h);
        self.markdown.hash(&mut h);
        for col in [self.text, self.muted, self.heading, self.code, self.code_bg, self.link, self.quote, self.accent] {
            col.to_array().hash(&mut h);
        }
        h.finish()
    }

    fn fmt(&self, color: Color32) -> TextFormat {
        TextFormat {
            font_id: self.font.clone(),
            color,
            line_height: Some(self.font.size * self.line_spacing),
            ..Default::default()
        }
    }

    fn scaled(&self, color: Color32, scale: f32) -> TextFormat {
        let size = self.font.size * scale;
        TextFormat {
            font_id: FontId::new(size, self.font.family.clone()),
            color,
            line_height: Some(size * self.line_spacing),
            ..Default::default()
        }
    }
}

/// 마크다운이 아니거나 너무 큰 파일은 강조 없이 그대로 배치합니다.
const MAX_HIGHLIGHT_BYTES: usize = 2_000_000;

pub fn highlight(text: &str, st: &HighlightStyle) -> LayoutJob {
    let mut job = LayoutJob::default();
    if !st.markdown || text.len() > MAX_HIGHLIGHT_BYTES {
        job.append(text, 0.0, st.fmt(st.text));
        return job;
    }

    let mut in_fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let trimmed = body.trim_start();

        // ``` 코드 블록
        let fence = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f));
        if let Some(open) = in_fence {
            let mut f = st.fmt(st.code);
            f.background = st.code_bg;
            job.append(line, 0.0, f);
            if fence == Some(open) {
                in_fence = None;
            }
            continue;
        }
        if let Some(f) = fence {
            in_fence = Some(f);
            let mut fm = st.fmt(st.muted);
            fm.background = st.code_bg;
            job.append(line, 0.0, fm);
            continue;
        }

        // -# 작은 글씨: 기호는 강조색, 내용은 흐리게
        if let Some(rest) = crate::live::subtext_body(trimmed) {
            let marker_end = body.len() - rest.len();
            job.append(&body[..marker_end], 0.0, st.fmt(st.accent));
            let base = st.fmt(st.muted);
            highlight_inline(&mut job, rest, st, &base, false);
            job.append(nl, 0.0, base);
            continue;
        }

        // # 제목
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].chars().next().is_none_or(|ch| ch == ' ') {
            let scale = match hashes {
                1 => 1.45,
                2 => 1.3,
                3 => 1.15,
                _ => 1.05,
            };
            let indent = body.len() - trimmed.len();
            let marker_end = indent + hashes;
            job.append(&body[..marker_end], 0.0, st.scaled(st.muted, scale));
            job.append(&body[marker_end..], 0.0, st.scaled(st.heading, scale));
            job.append(nl, 0.0, st.scaled(st.text, scale));
            continue;
        }

        // --- 구분선
        let compact: String = trimmed.chars().filter(|ch| !ch.is_whitespace()).collect();
        if compact.len() >= 3
            && (compact.chars().all(|ch| ch == '-')
                || compact.chars().all(|ch| ch == '*')
                || compact.chars().all(|ch| ch == '_'))
        {
            job.append(line, 0.0, st.fmt(st.muted));
            continue;
        }

        let mut rest_start = body.len() - trimmed.len();
        job.append(&body[..rest_start], 0.0, st.fmt(st.text));
        let mut base = st.fmt(st.text);

        // > 인용
        if trimmed.starts_with('>') {
            let n = trimmed.bytes().take_while(|&b| b == b'>' || b == b' ').count();
            job.append(&body[rest_start..rest_start + n], 0.0, st.fmt(st.muted));
            rest_start += n;
            base = st.fmt(st.quote);
            base.italics = true;
        } else if let Some(marker_len) = list_marker_len(trimmed) {
            // - 목록, 1. 번호 목록, - [ ] 할 일
            job.append(&body[rest_start..rest_start + marker_len], 0.0, st.fmt(st.accent));
            rest_start += marker_len;
            let after = &body[rest_start..];
            for task in ["[ ] ", "[x] ", "[X] "] {
                if after.starts_with(task) {
                    job.append(&body[rest_start..rest_start + task.len()], 0.0, st.fmt(st.accent));
                    rest_start += task.len();
                    if task != "[ ] " {
                        base = st.fmt(st.muted);
                    }
                    break;
                }
            }
        }

        let table = trimmed.starts_with('|');
        highlight_inline(&mut job, &body[rest_start..], st, &base, table);
        job.append(nl, 0.0, base);
    }
    job
}

/// "- ", "* ", "+ ", "12. ", "3) " 의 길이
fn list_marker_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return Some(2);
    }
    let digits = b.iter().take_while(|x| x.is_ascii_digit()).count();
    if (1..=9).contains(&digits) && b.len() > digits + 1 && matches!(b[digits], b'.' | b')') && b[digits + 1] == b' ' {
        return Some(digits + 2);
    }
    None
}

fn highlight_inline(job: &mut LayoutJob, s: &str, st: &HighlightStyle, base: &TextFormat, table: bool) {
    let muted = {
        let mut f = base.clone();
        f.color = st.muted;
        f.italics = false;
        f
    };
    let mut plain_start = 0;
    let mut i = 0;
    let bytes = s.as_bytes();

    let flush = |job: &mut LayoutJob, from: usize, to: usize| {
        if from < to {
            job.append(&s[from..to], 0.0, base.clone());
        }
    };

    while i < bytes.len() {
        let rest = &s[i..];
        let b = bytes[i];

        // `코드`
        if b == b'`' {
            let ticks = rest.bytes().take_while(|&x| x == b'`').count();
            let fence = &rest[..ticks];
            if let Some(close) = rest[ticks..].find(fence) {
                flush(job, plain_start, i);
                let end = i + ticks + close + ticks;
                let mut f = base.clone();
                f.color = st.code;
                f.background = st.code_bg;
                f.italics = false;
                job.append(&s[i..end], 0.0, f);
                i = end;
                plain_start = i;
                continue;
            }
        }

        // **굵게**, __굵게__, ~~취소선~~
        let mut matched = false;
        for (marker, strike) in [("**", false), ("__", false), ("~~", true)] {
            if rest.starts_with(marker)
                && let Some(close) = rest[2..].find(marker)
                && close > 0
            {
                flush(job, plain_start, i);
                let inner_end = i + 2 + close;
                job.append(marker, 0.0, muted.clone());
                let mut f = base.clone();
                if strike {
                    // 줄 간격을 넓히면 취소선이 글자 아래로 내려가 밑줄처럼 보이므로 흐린 색으로 표시
                    f.color = st.muted;
                } else {
                    f.color = st.heading;
                }
                job.append(&s[i + 2..inner_end], 0.0, f);
                job.append(marker, 0.0, muted.clone());
                i = inner_end + 2;
                plain_start = i;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        // *기울임*, _기울임_
        if (b == b'*' || b == b'_') && !rest.starts_with("**") && !rest.starts_with("__") {
            let word_boundary = b == b'*' || i == 0 || !s[..i].chars().last().is_some_and(char::is_alphanumeric);
            if word_boundary
                && let Some(close) = rest[1..].find(b as char)
                && close > 0
                && !rest[1..].starts_with(' ')
            {
                flush(job, plain_start, i);
                let inner_end = i + 1 + close;
                job.append(&s[i..i + 1], 0.0, muted.clone());
                let mut f = base.clone();
                f.italics = true;
                job.append(&s[i + 1..inner_end], 0.0, f);
                job.append(&s[inner_end..inner_end + 1], 0.0, muted.clone());
                i = inner_end + 1;
                plain_start = i;
                continue;
            }
        }

        // [링크](주소), ![이미지](경로)
        if b == b'[' || rest.starts_with("![") {
            let open = if b == b'!' { 1 } else { 0 };
            if let Some(close_br) = rest[open..].find("](") {
                let label_end = open + close_br;
                if let Some(close_par) = rest[label_end + 2..].find(')') {
                    flush(job, plain_start, i);
                    let url_end = label_end + 2 + close_par;
                    job.append(&rest[..open + 1], 0.0, muted.clone());
                    let mut f = base.clone();
                    f.color = st.link;
                    job.append(&rest[open + 1..label_end], 0.0, f);
                    job.append(&rest[label_end..url_end + 1], 0.0, muted.clone());
                    i += url_end + 1;
                    plain_start = i;
                    continue;
                }
            }
        }

        // 맨 주소 https://...
        if rest.starts_with("http://") || rest.starts_with("https://") {
            flush(job, plain_start, i);
            let len = rest.find(|ch: char| ch.is_whitespace() || ch == ')' || ch == '>').unwrap_or(rest.len());
            let mut f = base.clone();
            f.color = st.link;
            job.append(&rest[..len], 0.0, f);
            i += len;
            plain_start = i;
            continue;
        }

        // 표의 | 구분자
        if table && b == b'|' {
            flush(job, plain_start, i);
            job.append("|", 0.0, muted.clone());
            i += 1;
            plain_start = i;
            continue;
        }

        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    flush(job, plain_start, bytes.len());
}

// ---------------------------------------------------------------------------
// 그림 경로

pub fn file_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") }
}

pub fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
            let hex = |x: u8| (x as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(b[i + 1]) * 16 + hex(b[i + 2]));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

fn md_options() -> pulldown_cmark::Options {
    use pulldown_cmark::Options;
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_SMART_PUNCTUATION
}

// ---------------------------------------------------------------------------
// HTML 내보내기

/// 마크다운에 없는 우리 문법을 HTML 로 바꿔 둡니다:
/// 크기·위치를 지정한 그림 줄은 `<img>`, `-# 작은 글씨` 줄은 `<small>`.
fn extensions_to_html(text: &str) -> String {
    use crate::live::{ImgAlign, parse_image_line, subtext_body};
    use crate::media::MediaKind;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim_start();
        if let Some(rest) = subtext_body(trimmed) {
            // 안의 **굵게** 등은 그대로 마크다운으로 처리되고, 줄 끝 공백 두 칸으로 줄을 나눔
            out.push_str(&format!("<small class=\"subtext\">{rest}</small>  \n"));
            continue;
        }
        let kind = parse_image_line(trimmed).map(|img| crate::media::kind_of(img.dest));
        match parse_image_line(trimmed) {
            Some(img) if img.width.is_some() || img.align != ImgAlign::Left || kind != Some(MediaKind::Image) => {
                let align = match img.align {
                    ImgAlign::Left => "left",
                    ImgAlign::Center => "center",
                    ImgAlign::Right => "right",
                };
                let src = html_escape(&img.dest.replace(' ', "%20"));
                let alt = html_escape(img.alt);
                let element = match kind {
                    // 소리·영상은 브라우저의 기본 재생기로
                    Some(MediaKind::Audio) => {
                        let style = img.width.map(|w| format!(" style=\"width:{w}%\"")).unwrap_or_default();
                        format!("<audio controls preload=\"metadata\" src=\"{src}\" title=\"{alt}\"{style}></audio>")
                    }
                    Some(MediaKind::Video) => {
                        let width = img
                            .width
                            .map_or_else(|| " style=\"max-width:100%\"".to_owned(), |w| format!(" width=\"{w}%\""));
                        format!("<video controls preload=\"metadata\" src=\"{src}\" title=\"{alt}\"{width}></video>")
                    }
                    _ => {
                        let width = img.width.map(|w| format!(" width=\"{w}%\"")).unwrap_or_default();
                        format!("<img src=\"{src}\" alt=\"{alt}\"{width}>")
                    }
                };
                out.push_str(&format!("<p style=\"text-align:{align}\">{element}</p>\n"));
            }
            _ => out.push_str(line),
        }
    }
    out
}

pub fn to_html(text: &str, title: &str, palette: &Palette) -> String {
    let text = extensions_to_html(text);
    let parser = pulldown_cmark::Parser::new_ext(&text, md_options());
    let mut body = String::new();
    pulldown_cmark::html::push_html(&mut body, parser);
    let hexc = |rgb: [u8; 3]| format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    let code_bg = mix(c(palette.editor_bg), c(palette.text), 0.07);
    format!(
        r#"<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  body {{ max-width: 820px; margin: 40px auto; padding: 0 20px; line-height: 1.7;
         font-family: "Malgun Gothic", "Apple SD Gothic Neo", "Noto Sans KR", sans-serif;
         background: {bg}; color: {fg}; }}
  h1, h2, h3, h4, h5, h6 {{ color: {heading}; line-height: 1.3; }}
  h1, h2 {{ border-bottom: 1px solid {muted}55; padding-bottom: .3em; }}
  a {{ color: {link}; }}
  code {{ background: {code_bg}; color: {code}; padding: .15em .35em; border-radius: 4px;
         font-family: Consolas, "D2Coding", monospace; }}
  pre {{ background: {code_bg}; padding: 14px 16px; border-radius: 8px; overflow: auto; }}
  pre code {{ background: none; padding: 0; }}
  blockquote {{ margin: 0; padding: 0 1em; border-left: 4px solid {accent}; color: {quote}; }}
  table {{ border-collapse: collapse; }}
  th, td {{ border: 1px solid {muted}88; padding: 6px 12px; }}
  img {{ max-width: 100%; }}
  hr {{ border: none; border-top: 1px solid {muted}88; }}
  small.subtext {{ color: {muted}; font-size: .82em; }}
</style>
</head>
<body>
{body}
</body>
</html>
"#,
        lang = crate::i18n::lang().code(),
        title = html_escape(title),
        bg = hexc(palette.editor_bg),
        fg = hexc(palette.text),
        heading = hexc(palette.heading),
        muted = hexc(palette.muted),
        link = hexc(palette.link),
        code = hexc(palette.code),
        code_bg = format!("#{:02x}{:02x}{:02x}", code_bg.r(), code_bg.g(), code_bg.b()),
        accent = hexc(palette.accent),
        quote = hexc(palette.quote),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// 그림을 문서 폴더에 함께 보관

/// 문서가 가리키는 그림 중 `doc_dir` 밖에 있는 파일을 `doc_dir/folder` 로 복사하고
/// 링크를 그 복사본으로 바꾼 새 텍스트를 돌려줍니다. 바꿀 것이 없으면 `None`.
///
/// `old_base`는 상대 경로를 풀 기준 폴더입니다 (다른 이름으로 저장해서 폴더가 바뀐 경우 예전 폴더).
pub fn collect_images(text: &str, old_base: Option<&Path>, doc_dir: &Path, folder: &str) -> Option<String> {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut search = 0;
    while let Some(found) = text[search..].find("![") {
        let start = search + found;
        search = start + 2;
        let Some(mid) = text[start..].find("](") else { break };
        if text[start..start + mid].contains('\n') {
            continue; // 같은 줄 안의 그림 문법만
        }
        let dest_start = start + mid + 2;
        let rest = &text[dest_start..];
        if rest.starts_with('\n') {
            continue;
        }
        let (raw_len, dest, bracketed) = if let Some(inner) = rest.strip_prefix('<') {
            let Some(end) = inner.find('>') else { continue };
            (end + 2, &inner[..end], true)
        } else {
            let end = rest.find(|c: char| c.is_whitespace() || c == ')').unwrap_or(rest.len());
            (end, &rest[..end], false)
        };
        if dest.is_empty() || dest.contains("://") || dest.starts_with("data:") {
            continue;
        }
        let decoded = percent_decode(dest);
        let p = Path::new(&decoded);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(base) = old_base {
            base.join(p)
        } else {
            continue;
        };
        let new_rel = if let Ok(inside) = abs.strip_prefix(doc_dir) {
            // 이미 문서 폴더 안: 상대 경로가 맞으면 그대로
            if !p.is_absolute() && old_base == Some(doc_dir) {
                continue;
            }
            inside.to_path_buf()
        } else if crate::media::kind_of(dest) != crate::media::MediaKind::Image {
            // 소리·영상은 커서 복사하지 않습니다. 상대 경로였으면 문서를 옮겨도 찾을 수 있게 절대 경로로.
            if p.is_absolute() || old_base == Some(doc_dir) {
                continue;
            }
            abs.clone()
        } else {
            if !abs.is_file() {
                continue;
            }
            let target_dir = doc_dir.join(folder);
            if std::fs::create_dir_all(&target_dir).is_err() {
                continue;
            }
            let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image.png".into());
            let same_size =
                |t: &Path| std::fs::metadata(t).ok().map(|m| m.len()) == std::fs::metadata(&abs).ok().map(|m| m.len());
            let mut target = target_dir.join(&name);
            let mut n = 2;
            while target.exists() && !same_size(&target) {
                let (stem, ext) = match name.rsplit_once('.') {
                    Some((a, b)) => (a.to_owned(), format!(".{b}")),
                    None => (name.clone(), String::new()),
                };
                target = target_dir.join(format!("{stem} {n}{ext}"));
                n += 1;
            }
            if !target.exists() && std::fs::copy(&abs, &target).is_err() {
                continue;
            }
            Path::new(folder).join(target.file_name().unwrap_or_default())
        };
        let rel = new_rel.to_string_lossy().replace('\\', "/");
        let replacement = if bracketed || rel.contains([' ', '(', ')']) { format!("<{rel}>") } else { rel };
        edits.push((dest_start..dest_start + raw_len, replacement));
    }
    if edits.is_empty() {
        return None;
    }
    let mut out = text.to_owned();
    for (r, rep) in edits.into_iter().rev() {
        out.replace_range(r, &rep);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// 서식 도구

#[derive(Clone, Debug, PartialEq)]
pub enum MdAction {
    Bold,
    Italic,
    Strike,
    Code,
    Link,
    Heading(usize),
    Bullet,
    Numbered,
    Task,
    Quote,
    /// `-# ` 작은 글씨
    Subtext,
    CodeBlock,
    Table,
    Rule,
    /// 선택 영역을 이 텍스트로 바꿉니다.
    Insert(String),
}

pub fn char_to_byte(text: &str, ci: usize) -> usize {
    text.char_indices().nth(ci).map_or(text.len(), |(b, _)| b)
}

pub fn byte_to_char(text: &str, bi: usize) -> usize {
    text[..bi.min(text.len())].chars().count()
}

/// `sel`(글자 단위 범위)에 서식을 적용하고, 적용 후 선택할 범위(글자 단위)를 돌려줍니다.
pub fn apply_action(text: &mut String, sel: Range<usize>, action: &MdAction) -> Range<usize> {
    let start = char_to_byte(text, sel.start);
    let end = char_to_byte(text, sel.end);
    let b = match action {
        MdAction::Bold => wrap(text, start, end, "**", "**", t!("굵은 글씨")),
        MdAction::Italic => wrap(text, start, end, "*", "*", t!("기울임")),
        MdAction::Strike => wrap(text, start, end, "~~", "~~", t!("취소선")),
        MdAction::Code => wrap(text, start, end, "`", "`", "code"),
        MdAction::Link => {
            let label = if start == end { t!("링크 텍스트").to_owned() } else { text[start..end].to_owned() };
            let ins = format!("[{label}](https://)");
            text.replace_range(start..end, &ins);
            // 주소 부분을 선택해서 바로 입력할 수 있게
            let url_start = start + label.len() + 3;
            url_start..url_start + "https://".len()
        }
        MdAction::Heading(level) => {
            let level = *level;
            map_lines(text, start, end, |line, _| {
                let stripped = line.trim_start_matches('#').trim_start();
                let current = line.len() - line.trim_start_matches('#').len();
                if current == level { stripped.to_owned() } else { format!("{} {stripped}", "#".repeat(level)) }
            })
        }
        MdAction::Bullet => toggle_prefix(text, start, end, "- "),
        MdAction::Task => toggle_prefix(text, start, end, "- [ ] "),
        MdAction::Quote => toggle_prefix(text, start, end, "> "),
        MdAction::Subtext => toggle_prefix(text, start, end, "-# "),
        MdAction::Numbered => {
            let lines = line_span(text, start, end);
            let all_numbered = text[lines].split('\n').all(|l| numbered_prefix_len(l).is_some());
            map_lines(text, start, end, |line, n| {
                if all_numbered {
                    line[numbered_prefix_len(line).unwrap_or(0)..].to_owned()
                } else {
                    format!("{}. {line}", n + 1)
                }
            })
        }
        MdAction::CodeBlock => {
            let lines = line_span(text, start, end);
            let inner = text[lines.clone()].to_owned();
            let ins = format!("```\n{inner}\n```");
            text.replace_range(lines.clone(), &ins);
            let s = lines.start + 4;
            s..s + inner.len()
        }
        MdAction::Table => {
            let tpl = t!("| 제목 1 | 제목 2 | 제목 3 |\n| --- | --- | --- |\n| 내용 | 내용 | 내용 |\n");
            let r = insert_block(text, start, end, tpl);
            // 첫 칸("제목 1")을 골라 둬서 바로 고쳐 쓰게
            match text[start..r.end].find(t!("제목 1")) {
                Some(i) => start + i..start + i + t!("제목 1").len(),
                None => r,
            }
        }
        MdAction::Rule => insert_block(text, start, end, "---\n"),
        MdAction::Insert(s) => {
            text.replace_range(start..end, s);
            let e = start + s.len();
            e..e
        }
    };
    byte_to_char(text, b.start)..byte_to_char(text, b.end)
}

fn wrap(text: &mut String, start: usize, end: usize, pre: &str, post: &str, placeholder: &str) -> Range<usize> {
    // 이미 감싸져 있으면 풀기
    if text[..start].ends_with(pre) && text[end..].starts_with(post) && start != end {
        text.replace_range(end..end + post.len(), "");
        text.replace_range(start - pre.len()..start, "");
        return start - pre.len()..end - pre.len();
    }
    let inner = if start == end { placeholder.to_owned() } else { text[start..end].to_owned() };
    let ins = format!("{pre}{inner}{post}");
    text.replace_range(start..end, &ins);
    start + pre.len()..start + pre.len() + inner.len()
}

/// 선택 영역이 걸친 줄 전체의 바이트 범위 (마지막 줄바꿈 제외)
fn line_span(text: &str, start: usize, end: usize) -> Range<usize> {
    let ls = text[..start].rfind('\n').map_or(0, |p| p + 1);
    let le = text[end..].find('\n').map_or(text.len(), |p| end + p);
    ls..le.max(ls)
}

fn map_lines(text: &mut String, start: usize, end: usize, f: impl Fn(&str, usize) -> String) -> Range<usize> {
    let span = line_span(text, start, end);
    let new: Vec<String> = text[span.clone()].split('\n').enumerate().map(|(n, l)| f(l, n)).collect();
    let joined = new.join("\n");
    text.replace_range(span.clone(), &joined);
    span.start..span.start + joined.len()
}

fn toggle_prefix(text: &mut String, start: usize, end: usize, prefix: &str) -> Range<usize> {
    let span = line_span(text, start, end);
    let all = text[span.clone()].split('\n').all(|l| l.starts_with(prefix));
    map_lines(text, start, end, |line, _| if all { line[prefix.len()..].to_owned() } else { format!("{prefix}{line}") })
}

fn numbered_prefix_len(line: &str) -> Option<usize> {
    let d = line.bytes().take_while(u8::is_ascii_digit).count();
    (d > 0 && line[d..].starts_with(". ")).then_some(d + 2)
}

/// 블록(표, 구분선)을 빈 줄로 감싸 삽입
fn insert_block(text: &mut String, start: usize, end: usize, block: &str) -> Range<usize> {
    let mut ins = String::new();
    if start > 0 && !text[..start].ends_with("\n\n") {
        ins.push_str(if text[..start].ends_with('\n') { "\n" } else { "\n\n" });
    }
    ins.push_str(block);
    if end < text.len() && !text[end..].starts_with('\n') {
        ins.push('\n');
    }
    text.replace_range(start..end, &ins);
    let e = start + ins.len();
    e..e
}

// ---------------------------------------------------------------------------
// 찾기

/// 대소문자 구분 여부에 따라 `needle`이 나오는 모든 바이트 범위
pub fn find_all(haystack: &str, needle: &str, match_case: bool) -> Vec<Range<usize>> {
    if needle.is_empty() {
        return Vec::new();
    }
    if match_case {
        return haystack.match_indices(needle).map(|(i, m)| i..i + m.len()).collect();
    }
    // 소문자로 접은 문자열에서 찾고, 원래 위치로 되돌립니다.
    let (folded, map) = fold(haystack);
    let (needle, _) = fold(needle);
    folded.match_indices(needle.as_str()).map(|(i, m)| map[i]..map[i + m.len()]).filter(|r| r.start < r.end).collect()
}

fn fold(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len() + 1);
    for (bi, ch) in s.char_indices() {
        for lc in ch.to_lowercase() {
            let before = out.len();
            out.push(lc);
            map.extend(std::iter::repeat_n(bi, out.len() - before));
        }
    }
    map.push(s.len());
    (out, map)
}

/// 상태 표시줄용 단어 수
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_toggles() {
        let mut t = "안녕 세상".to_owned();
        let r = apply_action(&mut t, 3..5, &MdAction::Bold);
        assert_eq!(t, "안녕 **세상**");
        assert_eq!(r, 5..7);
        let r = apply_action(&mut t, r, &MdAction::Bold);
        assert_eq!(t, "안녕 세상");
        assert_eq!(r, 3..5);
    }

    #[test]
    fn bullet_toggles_lines() {
        let mut t = "a\nb\nc".to_owned();
        apply_action(&mut t, 0..3, &MdAction::Bullet);
        assert_eq!(t, "- a\n- b\nc");
        apply_action(&mut t, 0..7, &MdAction::Bullet);
        assert_eq!(t, "a\nb\nc");
    }

    #[test]
    fn heading_switches_level() {
        let mut t = "## 제목".to_owned();
        apply_action(&mut t, 0..0, &MdAction::Heading(1));
        assert_eq!(t, "# 제목");
        apply_action(&mut t, 0..0, &MdAction::Heading(1));
        assert_eq!(t, "제목");
    }

    #[test]
    fn numbered_list() {
        let mut t = "x\ny".to_owned();
        apply_action(&mut t, 0..3, &MdAction::Numbered);
        assert_eq!(t, "1. x\n2. y");
        apply_action(&mut t, 0..9, &MdAction::Numbered);
        assert_eq!(t, "x\ny");
    }

    #[test]
    fn find_case_insensitive_korean() {
        let hits = find_all("Hello 한글 HELLO hello", "hello", false);
        assert_eq!(hits.len(), 3);
        assert_eq!(&"Hello 한글 HELLO hello"[hits[1].clone()], "HELLO");
    }

    #[test]
    fn media_lines_export_as_players() {
        let html = to_html(
            "![노래](<media/a b.mp3>)\n\n![영상](v.mp4 \"width=50% align=center\")\n",
            "t",
            &crate::settings::Settings::default().palette(),
        );
        assert!(html.contains("<audio controls preload=\"metadata\" src=\"media/a%20b.mp3\""), "{html}");
        assert!(html.contains("<p style=\"text-align:center\"><video controls"), "{html}");
        assert!(html.contains("width=\"50%\"></video>"), "{html}");
        assert!(!html.contains("<img"), "{html}");
    }

    #[test]
    fn collect_leaves_media_files_alone() {
        let root = std::env::temp_dir().join(format!("tbn_media_collect_{}", std::process::id()));
        let (old, new) = (root.join("old"), root.join("new"));
        std::fs::create_dir_all(old.join("media")).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("media").join("song.mp3"), b"x").unwrap();
        let text = "![노래](media/song.mp3)\n";
        // 같은 폴더면 그대로
        assert_eq!(collect_images(text, Some(&old), &old, "images"), None);
        // 다른 폴더로 옮기면 복사하지 않고 원래 파일을 절대 경로로 가리킴
        let out = collect_images(text, Some(&old), &new, "images").unwrap();
        assert!(out.contains("song.mp3"), "{out}");
        assert!(!new.join("images").exists(), "소리 파일을 복사하면 안 됨");
        let dest = crate::live::parse_image_line(out.trim()).unwrap().dest.to_owned();
        assert!(Path::new(&dest).is_absolute(), "{dest}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn collects_outside_images_into_doc_folder() {
        let tmp = std::env::temp_dir().join(format!("tbn_collect_{}", std::process::id()));
        let outside = tmp.join("outside");
        let doc_dir = tmp.join("notes");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&doc_dir).unwrap();
        let img = outside.join("my pic.png");
        std::fs::write(&img, b"png-bytes").unwrap();
        let abs = img.to_string_lossy().replace('\\', "/");
        let text = format!("# 제목\n![사진](<{abs}> \"width=50%\")\n![웹](https://x/y.png)\n");
        let out = collect_images(&text, None, &doc_dir, "images").expect("바뀌어야 함");
        assert!(out.contains("![사진](<images/my pic.png> \"width=50%\")"), "{out}");
        assert!(out.contains("https://x/y.png"));
        assert!(doc_dir.join("images/my pic.png").is_file());
        // 두 번째 저장에서는 바꿀 것이 없음
        assert_eq!(collect_images(&out, Some(&doc_dir), &doc_dir, "images"), None);
        // 다른 폴더로 "다른 이름으로 저장": 예전 폴더 기준 상대 경로 그림도 따라옴
        let new_dir = tmp.join("moved");
        std::fs::create_dir_all(&new_dir).unwrap();
        let moved = collect_images(&out, Some(&doc_dir), &new_dir, "images").unwrap();
        assert!(moved.contains("<images/my pic.png>"));
        assert!(new_dir.join("images/my pic.png").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn subtext_toggles_and_exports() {
        let mut t = "참고\n둘째".to_owned();
        apply_action(&mut t, 0..4, &MdAction::Subtext);
        assert_eq!(t, "-# 참고\n-# 둘째");
        apply_action(&mut t, 0..10, &MdAction::Subtext);
        assert_eq!(t, "참고\n둘째");
        let html = to_html("본문\n\n-# **작은** 글씨\n", "t", &crate::settings::Settings::default().palette());
        assert!(html.contains(r#"<small class="subtext"><strong>작은</strong> 글씨</small>"#), "{html}");
    }

    #[test]
    fn html_keeps_image_size() {
        let html =
            to_html("![a](b.png \"width=40% align=center\")\n", "t", &crate::settings::Settings::default().palette());
        assert!(html.contains(r#"<p style="text-align:center"><img src="b.png" alt="a" width="40%"></p>"#), "{html}");
    }

    #[test]
    fn highlight_keeps_all_text() {
        let style = HighlightStyle::new(
            &crate::settings::Palette::preset(
                crate::settings::ThemePreset::Dark,
                &crate::settings::Settings::default().custom_palette,
            ),
            FontId::monospace(14.0),
            1.2,
            true,
        );
        let src = "# 제목\n\n**굵게** _기울임_ `코드` [링크](http://x) ~~취소~~\n- [x] 완료\n> 인용\n```\ncode **no**\n```\n| a | b |\n---\nhttps://example.com 끝 * 별 _밑줄";
        let job = highlight(src, &style);
        assert_eq!(job.text, src);
    }
}
