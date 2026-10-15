//! 문서 모드: 마크다운 기호를 숨기고 서식을 바로 보여 주는 편집 화면 (Typora 방식).
//!
//! 글자 자체는 그대로 두고(그래야 TextEdit이 그대로 편집할 수 있음) 모양만 바꿉니다.
//! - `**`, `#`, `[ ](url)` 같은 기호는 커서가 있는 줄에서만 흐리게 보이고, 다른 줄에서는 숨깁니다.
//! - 목록 점, 체크박스, 인용 막대, 구분선, 이미지, 코드 블록 배경은 글자 대신 [`Deco`]로 돌려주고
//!   편집기가 그 위치에 직접 그립니다.

use std::hash::{Hash, Hasher};
use std::path::Path;

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontFamily, FontId, Stroke, Vec2};

use crate::media::MediaKind;

/// 글자 위치는 모두 "글자 단위" 인덱스 (TextEdit 커서와 같은 단위)
#[derive(Clone, Debug, PartialEq)]
pub enum Deco {
    /// 목록의 `-` 자리에 점
    Bullet { at: usize, level: usize },
    /// `[ ]` 자리에 체크박스. `byte`는 원문에서 공백/x 가 있는 바이트 위치
    Task { at: usize, checked: bool, byte: usize },
    /// 인용 줄 왼쪽 막대
    Quote { start: usize, end: usize },
    /// `---` 구분선
    Rule { at: usize },
    /// 그림·소리·영상만 있는 줄. `top`은 줄 맨 위에서 그림까지의 간격,
    /// `line`은 원문에서 이 줄의 바이트 범위, `max_w`는 배치할 때 쓴 최대 너비 (크기 조절용)
    /// `at`..`end` 는 이 줄의 글자 범위 (커서는 그림 앞 `at` 이나 뒤 `end` 에만 섭니다).
    /// 소리·영상(`kind`)은 그림 대신 재생 카드를 그립니다.
    Image {
        kind: MediaKind,
        at: usize,
        end: usize,
        uri: String,
        size: Vec2,
        top: f32,
        align: ImgAlign,
        line: std::ops::Range<usize>,
        max_w: f32,
    },
    /// 코드 블록 배경
    CodeBlock { start: usize, end: usize },
    /// Ctrl+클릭으로 열 링크
    Link { start: usize, end: usize, url: String },
}

#[derive(Clone)]
pub struct LiveStyle {
    pub size: f32,
    pub body: FontFamily,
    pub bold: FontFamily,
    pub mono: FontFamily,
    pub text: Color32,
    pub muted: Color32,
    pub heading: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub link: Color32,
    pub quote: Color32,
}

impl LiveStyle {
    pub fn hash_into(&self, h: &mut impl Hasher) {
        self.size.to_bits().hash(h);
        self.body.hash(h);
        self.bold.hash(h);
        self.mono.hash(h);
        for c in [self.text, self.muted, self.heading, self.code, self.code_bg, self.link, self.quote] {
            c.to_array().hash(h);
        }
    }
}

pub struct LiveLayout {
    pub job: LayoutJob,
    pub decos: Vec<Deco>,
    /// 이 문서가 쓰는 그림 주소 (불러오기가 끝나면 다시 배치하려고 기억)
    pub image_uris: Vec<String>,
}

pub struct LiveInput<'a> {
    pub text: &'a str,
    pub style: &'a LiveStyle,
    /// 기호를 드러낼 줄 (커서가 있는 줄, 0부터)
    pub active_line: Option<usize>,
    pub base_dir: Option<&'a Path>,
    /// 그림 원래 크기 (아직 못 불러왔으면 `None`)
    pub image_size: &'a dyn Fn(&str) -> Option<Vec2>,
    pub max_image_width: f32,
}

const MAX_LIVE_BYTES: usize = 1_000_000;

/// 소리 카드 높이
pub const AUDIO_CARD_HEIGHT: f32 = 58.0;

/// `-#` 작은 글씨의 크기 (본문 대비)
const SUBTEXT_SCALE: f32 = 0.8;

/// 줄이 `-# 작은 글씨` 면 기호 뒤의 내용 (디스코드의 subtext 문법)
pub fn subtext_body(trimmed: &str) -> Option<&str> {
    if trimmed == "-#" {
        return Some("");
    }
    trimmed.strip_prefix("-# ")
}

struct Builder<'a> {
    job: LayoutJob,
    st: &'a LiveStyle,
    chars: usize,
    decos: Vec<Deco>,
}

impl<'a> Builder<'a> {
    fn push(&mut self, s: &str, f: TextFormat) {
        if !s.is_empty() {
            self.job.append(s, 0.0, f);
            self.chars += s.chars().count();
        }
    }

    fn fmt(&self, family: &FontFamily, scale: f32, color: Color32) -> TextFormat {
        TextFormat { font_id: FontId::new(self.st.size * scale, family.clone()), color, ..Default::default() }
    }

    fn body(&self) -> TextFormat {
        self.fmt(&self.st.body, 1.0, self.st.text)
    }

    /// 거의 폭이 없고 보이지 않는 글자
    fn hidden(&self) -> TextFormat {
        TextFormat {
            font_id: FontId::new(0.1, self.st.body.clone()),
            color: Color32::TRANSPARENT,
            ..Default::default()
        }
    }

    /// 커서 줄이면 흐리게, 아니면 숨김
    fn marker(&self, active: bool) -> TextFormat {
        if active { self.fmt(&self.st.body, 1.0, self.st.muted) } else { self.hidden() }
    }
}

pub fn layout(input: &LiveInput<'_>) -> LiveLayout {
    let st = input.style;
    let text = input.text;
    let mut b = Builder { job: LayoutJob::default(), st, chars: 0, decos: Vec::new() };
    let mut image_uris = Vec::new();

    if text.len() > MAX_LIVE_BYTES {
        let f = b.body();
        b.push(text, f);
        return LiveLayout { job: b.job, decos: b.decos, image_uris };
    }

    let mut fence: Option<(&str, usize)> = None; // (여는 기호, 블록 시작 글자 위치)
    for (line_no, line) in text.split_inclusive('\n').enumerate() {
        let active = input.active_line == Some(line_no);
        // 이 줄이 원문에서 시작하는 바이트 위치
        let line_start_byte = line.as_ptr() as usize - text.as_ptr() as usize;
        let (body, nl) = match line.strip_suffix('\n') {
            Some(x) => (x, "\n"),
            None => (line, ""),
        };
        let line_start_char = b.chars;
        let trimmed = body.trim_start();
        let indent = body.len() - trimmed.len();
        let is_fence = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f));

        // ``` 코드 블록
        if let Some((open, start)) = fence {
            let closing = is_fence == Some(open);
            if closing {
                let f = if active { b.fmt(&st.mono, 0.85, st.muted) } else { b.hidden() };
                b.push(body, f);
                let end = b.chars;
                b.decos.push(Deco::CodeBlock { start, end });
                fence = None;
                let f = if active { b.fmt(&st.mono, 0.85, st.muted) } else { b.hidden() };
                b.push(nl, f);
            } else {
                let f = b.fmt(&st.mono, 0.9, st.code);
                b.push(line, f);
            }
            continue;
        }
        if let Some(open) = is_fence {
            fence = Some((open, line_start_char));
            let f = if active { b.fmt(&st.mono, 0.85, st.muted) } else { b.hidden() };
            b.push(line, f);
            continue;
        }

        // 그림·소리·영상만 있는 줄: 경로 글자는 커서가 있어도 숨기고 그림(카드)만 보여 줍니다
        // (크기·위치·삭제는 그림을 눌러서 나오는 도구로).
        if let Some(img) = parse_image_line(trimmed) {
            let uri = resolve_uri(img.dest, input.base_dir);
            let kind = crate::media::kind_of(img.dest);
            let max_w = input.max_image_width.max(40.0);
            let pct_w = img.width.map(|pct| max_w * pct as f32 / 100.0);
            let size = match kind {
                MediaKind::Image => {
                    image_uris.push(uri.clone());
                    (input.image_size)(&uri).filter(|s| s.x > 0.0 && s.y > 0.0).map(|nat| {
                        let w = pct_w.unwrap_or(nat.x.min(max_w));
                        Vec2::new(w, nat.y * w / nat.x)
                    })
                }
                // 소리 카드는 너무 좁으면 재생 막대가 안 보여서 최소 폭을 둡니다
                MediaKind::Audio => Some(Vec2::new(pct_w.unwrap_or(440.0).max(300.0).min(max_w), AUDIO_CARD_HEIGHT)),
                MediaKind::Video => {
                    let w = pct_w.unwrap_or(480.0).max(160.0).min(max_w);
                    Some(Vec2::new(w, w * 9.0 / 16.0))
                }
            };
            if let Some(size) = size {
                let top = 6.0;
                let mut f = b.hidden();
                f.line_height = Some(top + size.y + 10.0);
                b.push(body, f.clone());
                b.push(nl, f);
                let line = line_start_byte..line_start_byte + body.len();
                let end = line_start_char + body.chars().count();
                b.decos.push(Deco::Image {
                    kind,
                    at: line_start_char,
                    end,
                    uri,
                    size,
                    top,
                    align: img.align,
                    line,
                    max_w,
                });
                continue;
            }
        }

        // -# 작은 글씨 (디스코드식 보조 글): 기호는 숨기고 작고 흐리게
        if let Some(rest) = subtext_body(trimmed) {
            let marker_end = body.len() - rest.len();
            let mut mf = b.marker(active);
            if active {
                mf.font_id.size = st.size * SUBTEXT_SCALE;
            }
            b.push(&body[..marker_end], mf);
            let sf = b.fmt(&st.body, SUBTEXT_SCALE, st.muted);
            inline(&mut b, rest, &sf, active);
            b.push(nl, sf);
            continue;
        }

        // # 제목
        let hashes = trimmed.bytes().take_while(|&c| c == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].chars().next().is_none_or(|c| c == ' ') {
            let scale = match hashes {
                1 => 1.75,
                2 => 1.45,
                3 => 1.22,
                _ => 1.08,
            };
            let marker_end = (indent + hashes + 1).min(body.len());
            let mut mf = b.marker(active);
            if active {
                mf.font_id.size = st.size * scale;
            }
            b.push(&body[..marker_end], mf);
            let hf = b.fmt(&st.bold, scale, st.heading);
            inline(&mut b, &body[marker_end..], &hf, active);
            b.push(nl, hf);
            continue;
        }

        // --- 구분선
        let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.len() >= 3 && ["-", "*", "_"].iter().any(|m| compact.chars().all(|c| c.to_string() == *m)) {
            if active {
                let f = b.fmt(&st.body, 1.0, st.muted);
                b.push(line, f);
            } else {
                let mut f = b.hidden();
                f.line_height = Some(st.size * 1.4);
                b.push(body, f.clone());
                b.push(nl, f);
                b.decos.push(Deco::Rule { at: line_start_char });
            }
            continue;
        }

        let mut rest = indent;
        let mut base = b.body();
        b.push(&body[..indent], base.clone());

        if trimmed.starts_with('>') {
            // > 인용
            let n = trimmed.bytes().take_while(|&c| c == b'>' || c == b' ').count();
            let f = b.marker(active);
            b.push(&body[rest..rest + n], f);
            rest += n;
            base = b.fmt(&st.body, 1.0, st.quote);
            let end_char = line_start_char + body.chars().count();
            b.decos.push(Deco::Quote { start: line_start_char, end: end_char });
        } else if let Some(m) = list_marker(trimmed) {
            let after = &body[rest + m.len..];
            let task = ["[ ] ", "[x] ", "[X] "].into_iter().find(|t| after.starts_with(t) || after == t.trim_end());
            match (m.kind, task) {
                (ListKind::Bullet, Some(t)) => {
                    // "- " 는 숨기고 "[ ]" 자리에 체크박스
                    let f = b.hidden();
                    b.push(&body[rest..rest + m.len], f);
                    rest += m.len;
                    let checked = t != "[ ] ";
                    let at = b.chars;
                    let box_len = 3.min(body.len() - rest);
                    let mut bf = b.body();
                    bf.color = Color32::TRANSPARENT;
                    b.push(&body[rest..rest + box_len], bf);
                    b.decos.push(Deco::Task { at, checked, byte: line_start_byte + rest + 1 });
                    rest += box_len;
                    if checked {
                        base = b.fmt(&st.body, 1.0, st.muted);
                        base.strikethrough = Stroke::new(1.0, st.muted);
                    }
                }
                (ListKind::Bullet, None) => {
                    let at = b.chars;
                    let mut f = b.body();
                    f.color = Color32::TRANSPARENT;
                    b.push(&body[rest..rest + m.len], f);
                    rest += m.len;
                    b.decos.push(Deco::Bullet { at, level: indent / 2 });
                }
                (ListKind::Number, _) => {
                    let f = b.fmt(&st.body, 1.0, st.muted);
                    b.push(&body[rest..rest + m.len], f);
                    rest += m.len;
                }
            }
        } else if trimmed.starts_with('|') {
            // 표: 고정폭으로 줄 맞춤
            base = b.fmt(&st.mono, 0.9, st.text);
            let pipe = b.fmt(&st.mono, 0.9, st.muted);
            for (i, cell) in body[rest..].split('|').enumerate() {
                if i > 0 {
                    b.push("|", pipe.clone());
                }
                b.push(cell, base.clone());
            }
            b.push(nl, base);
            continue;
        }

        inline(&mut b, &body[rest..], &base, active);
        b.push(nl, base);
    }
    if let Some((_, start)) = fence {
        let end = b.chars;
        b.decos.push(Deco::CodeBlock { start, end });
    }
    LiveLayout { job: b.job, decos: b.decos, image_uris }
}

enum ListKind {
    Bullet,
    Number,
}

struct ListMarker {
    kind: ListKind,
    len: usize,
}

fn list_marker(s: &str) -> Option<ListMarker> {
    let by = s.as_bytes();
    if by.len() >= 2 && matches!(by[0], b'-' | b'*' | b'+') && by[1] == b' ' {
        return Some(ListMarker { kind: ListKind::Bullet, len: 2 });
    }
    let d = by.iter().take_while(|x| x.is_ascii_digit()).count();
    if (1..=9).contains(&d) && by.len() > d + 1 && matches!(by[d], b'.' | b')') && by[d + 1] == b' ' {
        return Some(ListMarker { kind: ListKind::Number, len: d + 2 });
    }
    None
}

/// 그림 가로 위치
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImgAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// 한 줄 전체가 그림인 `![alt](dest "width=50% align=center")`
#[derive(Debug, PartialEq)]
pub struct ImageLine<'a> {
    pub alt: &'a str,
    /// 원문에 쓰인 그대로의 경로 (`<...>` 포함)
    pub dest_raw: &'a str,
    pub dest: &'a str,
    /// 본문 폭 대비 너비 (%). 없으면 원래 크기 (본문 폭을 넘지 않게)
    pub width: Option<u32>,
    pub align: ImgAlign,
}

pub fn parse_image_line(s: &str) -> Option<ImageLine<'_>> {
    let s = s.trim_end();
    let inner = s.strip_prefix("![")?.strip_suffix(')')?;
    let (alt, rest) = inner.split_once("](")?;
    let rest = rest.trim_start();
    let (dest_raw, dest, after) = if rest.starts_with('<') {
        let end = rest.find('>')?;
        (&rest[..=end], &rest[1..end], &rest[end + 1..])
    } else {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        (&rest[..end], &rest[..end], &rest[end..])
    };
    let after = after.trim();
    if dest.is_empty() || (!after.is_empty() && !(after.starts_with('"') && after.ends_with('"') && after.len() >= 2)) {
        return None;
    }
    let mut width = None;
    let mut align = ImgAlign::Left;
    for token in after.trim_matches('"').split_whitespace() {
        match token.split_once('=') {
            Some(("width", v)) => width = v.trim_end_matches('%').parse::<u32>().ok().map(|w| w.clamp(5, 100)),
            Some(("align", "center")) => align = ImgAlign::Center,
            Some(("align", "right")) => align = ImgAlign::Right,
            _ => {}
        }
    }
    Some(ImageLine { alt, dest_raw, dest, width, align })
}

/// 크기·위치를 바꾼 그림 줄을 다시 씁니다.
pub fn format_image_line(alt: &str, dest_raw: &str, width: Option<u32>, align: ImgAlign) -> String {
    let mut attrs = Vec::new();
    if let Some(w) = width {
        attrs.push(format!("width={w}%"));
    }
    match align {
        ImgAlign::Left => {}
        ImgAlign::Center => attrs.push("align=center".into()),
        ImgAlign::Right => attrs.push("align=right".into()),
    }
    if attrs.is_empty() {
        format!("![{alt}]({dest_raw})")
    } else {
        format!("![{alt}]({dest_raw} \"{}\")", attrs.join(" "))
    }
}

pub fn resolve_uri(dest: &str, base_dir: Option<&Path>) -> String {
    if dest.contains("://") || dest.starts_with("data:") {
        return dest.to_owned();
    }
    let decoded = crate::markdown::percent_decode(dest);
    let p = Path::new(&decoded);
    let abs = match base_dir {
        Some(dir) if !p.is_absolute() => dir.join(p),
        _ => p.to_path_buf(),
    };
    crate::markdown::file_uri(&abs)
}

/// 줄 안의 서식 (**굵게**, *기울임*, ~~취소~~, `코드`, [링크](주소), 맨 주소)
fn inline(b: &mut Builder<'_>, s: &str, base: &TextFormat, active: bool) {
    let st = b.st;
    let marker = {
        let mut m = b.marker(active);
        if active {
            m.font_id.size = base.font_id.size;
        }
        m
    };
    let bytes = s.as_bytes();
    let mut plain = 0;
    let mut i = 0;
    let flush = |b: &mut Builder<'_>, from: usize, to: usize| {
        if from < to {
            b.push(&s[from..to], base.clone());
        }
    };

    while i < bytes.len() {
        let rest = &s[i..];
        let c = bytes[i];

        if c == b'`' {
            let ticks = rest.bytes().take_while(|&x| x == b'`').count();
            if let Some(close) = rest[ticks..].find(&rest[..ticks]) {
                flush(b, plain, i);
                b.push(&rest[..ticks], marker.clone());
                let mut f = base.clone();
                f.font_id = FontId::new(base.font_id.size * 0.9, st.mono.clone());
                f.color = st.code;
                f.background = st.code_bg;
                f.strikethrough = Stroke::NONE;
                b.push(&rest[ticks..ticks + close], f);
                b.push(&rest[ticks + close..ticks * 2 + close], marker.clone());
                i += ticks * 2 + close;
                plain = i;
                continue;
            }
        }

        let mut matched = false;
        for (m, kind) in [("**", 0), ("__", 0), ("~~", 1)] {
            if rest.starts_with(m)
                && let Some(close) = rest[2..].find(m)
                && close > 0
            {
                flush(b, plain, i);
                b.push(m, marker.clone());
                let mut f = base.clone();
                if kind == 0 {
                    f.font_id.family = st.bold.clone();
                } else {
                    f.strikethrough = Stroke::new(1.0, base.color);
                }
                inline(b, &rest[2..2 + close], &f, active);
                b.push(m, marker.clone());
                i += close + 4;
                plain = i;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }

        if (c == b'*' || c == b'_') && !rest.starts_with("**") && !rest.starts_with("__") {
            let boundary = c == b'*' || i == 0 || !s[..i].chars().last().is_some_and(char::is_alphanumeric);
            if boundary
                && let Some(close) = rest[1..].find(c as char)
                && close > 0
                && !rest[1..].starts_with(' ')
            {
                flush(b, plain, i);
                b.push(&rest[..1], marker.clone());
                let mut f = base.clone();
                f.italics = true;
                inline(b, &rest[1..1 + close], &f, active);
                b.push(&rest[1 + close..2 + close], marker.clone());
                i += close + 2;
                plain = i;
                continue;
            }
        }

        if c == b'[' || rest.starts_with("![") {
            let open = usize::from(c == b'!');
            if let Some(cb) = rest[open..].find("](") {
                let label_end = open + cb;
                if let Some(cp) = rest[label_end + 2..].find(')') {
                    flush(b, plain, i);
                    let url_end = label_end + 2 + cp;
                    let url = rest[label_end + 2..url_end].trim().trim_start_matches('<').trim_end_matches('>');
                    b.push(&rest[..open + 1], marker.clone());
                    let mut f = base.clone();
                    f.color = st.link;
                    f.underline = Stroke::new(1.0, st.link);
                    let start = b.chars;
                    b.push(&rest[open + 1..label_end], f);
                    b.decos.push(Deco::Link { start, end: b.chars, url: url.to_owned() });
                    b.push(&rest[label_end..url_end + 1], marker.clone());
                    i += url_end + 1;
                    plain = i;
                    continue;
                }
            }
        }

        if rest.starts_with("http://") || rest.starts_with("https://") {
            flush(b, plain, i);
            let len = rest.find(|ch: char| ch.is_whitespace() || ch == ')' || ch == '>').unwrap_or(rest.len());
            let mut f = base.clone();
            f.color = st.link;
            f.underline = Stroke::new(1.0, st.link);
            let start = b.chars;
            b.push(&rest[..len], f);
            b.decos.push(Deco::Link { start, end: b.chars, url: rest[..len].to_owned() });
            i += len;
            plain = i;
            continue;
        }

        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    flush(b, plain, bytes.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> LiveStyle {
        LiveStyle {
            size: 16.0,
            body: FontFamily::Proportional,
            bold: FontFamily::Proportional,
            mono: FontFamily::Monospace,
            text: Color32::BLACK,
            muted: Color32::GRAY,
            heading: Color32::BLUE,
            code: Color32::RED,
            code_bg: Color32::LIGHT_GRAY,
            link: Color32::BLUE,
            quote: Color32::DARK_GRAY,
        }
    }

    fn run(text: &str, active: Option<usize>, img: Option<Vec2>) -> LiveLayout {
        let st = style();
        let f = move |_: &str| img;
        layout(&LiveInput {
            text,
            style: &st,
            active_line: active,
            base_dir: None,
            image_size: &f,
            max_image_width: 500.0,
        })
    }

    #[test]
    fn keeps_every_character() {
        let src = "# 제목\n**굵게** *기울임* ~~취소~~ `코드` [링크](http://x)\n- 항목\n  - 하위\n- [ ] 할 일\n- [x] 끝\n1. 번호\n> 인용\n---\n```rust\nlet a = 1;\n```\n| a | b |\n![그림](a.png)\n끝";
        for active in [None, Some(0), Some(13)] {
            for img in [None, Some(Vec2::new(800.0, 400.0))] {
                assert_eq!(run(src, active, img).job.text, src);
            }
        }
    }

    #[test]
    fn decorations_point_at_right_chars() {
        let src = "- 사과\n- [x] 끝\n---\n> 말\n";
        let out = run(src, None, None);
        let chars: Vec<char> = src.chars().collect();
        for d in &out.decos {
            match d {
                Deco::Bullet { at, .. } => assert_eq!(chars[*at], '-'),
                Deco::Task { at, checked, byte } => {
                    assert_eq!(chars[*at], '[');
                    assert!(*checked);
                    assert_eq!(&src[*byte..*byte + 1], "x");
                }
                Deco::Rule { at } => assert_eq!(chars[*at], '-'),
                Deco::Quote { start, .. } => assert_eq!(chars[*start], '>'),
                _ => {}
            }
        }
        assert_eq!(out.decos.len(), 4);
    }

    #[test]
    fn image_line_reserves_space_once_loaded() {
        let out = run("![고양이](cat.png)\n", None, Some(Vec2::new(1000.0, 500.0)));
        let Some(Deco::Image { size, .. }) = out.decos.first() else { panic!("{:?}", out.decos) };
        assert_eq!(*size, Vec2::new(500.0, 250.0));
        // 아직 못 불러온 그림은 링크처럼 보이다가, 불러오면 그림으로 바뀝니다.
        let pending = run("![고양이](cat.png)\n", None, None);
        assert!(!pending.decos.iter().any(|d| matches!(d, Deco::Image { .. })));
        assert_eq!(pending.image_uris.len(), 1);
    }

    #[test]
    fn subtext_lines() {
        assert_eq!(subtext_body("-# 작은 글씨"), Some("작은 글씨"));
        assert_eq!(subtext_body("-#"), Some(""));
        assert_eq!(subtext_body("-#붙은 글"), None);
        assert_eq!(subtext_body("- # 목록"), None);
        // 기호를 숨겨도 글자는 그대로
        let src = "-# **작은** 글씨";
        for active in [None, Some(0)] {
            assert_eq!(run(src, active, None).job.text, src);
        }
    }

    #[test]
    fn parses_image_lines() {
        let p = parse_image_line("![a](b.png)").unwrap();
        assert_eq!((p.dest, p.alt, p.width, p.align), ("b.png", "a", None, ImgAlign::Left));
        let p = parse_image_line("![a](<my pic.png>)").unwrap();
        assert_eq!((p.dest, p.dest_raw), ("my pic.png", "<my pic.png>"));
        let p = parse_image_line("![a](b.png \"width=40% align=center\")").unwrap();
        assert_eq!((p.width, p.align), (Some(40), ImgAlign::Center));
        assert!(parse_image_line("글 ![a](b.png)").is_none());
    }

    #[test]
    fn image_attrs_roundtrip() {
        let line = format_image_line("고양이", "<my cat.png>", Some(55), ImgAlign::Right);
        assert_eq!(line, "![고양이](<my cat.png> \"width=55% align=right\")");
        let p = parse_image_line(&line).unwrap();
        assert_eq!((p.alt, p.dest, p.width, p.align), ("고양이", "my cat.png", Some(55), ImgAlign::Right));
        assert_eq!(format_image_line("a", "b.png", None, ImgAlign::Left), "![a](b.png)");
    }

    #[test]
    fn media_lines_become_cards_without_loading() {
        // 그림 크기를 몰라도(불러오지 않아도) 소리·영상 카드는 바로 자리를 잡습니다
        let text = "앞
![노래](media/song.mp3)
![영상](<media/my clip.mp4> \"width=50%\")
뒤";
        let out = run(text, None, None);
        assert_eq!(out.job.text, text);
        assert!(out.image_uris.is_empty(), "소리·영상은 그림으로 불러오지 않음");
        let kinds: Vec<_> = out
            .decos
            .iter()
            .filter_map(|d| match d {
                Deco::Image { kind, size, at, end, .. } => Some((*kind, *size, *at, *end)),
                _ => None,
            })
            .collect();
        assert_eq!(kinds.len(), 2, "{:?}", out.decos);
        assert_eq!(kinds[0].0, MediaKind::Audio);
        assert_eq!(kinds[0].1.y, AUDIO_CARD_HEIGHT);
        assert_eq!(kinds[0].2, 2);
        assert_eq!(kinds[0].3, 2 + "![노래](media/song.mp3)".chars().count());
        assert_eq!(kinds[1].0, MediaKind::Video);
        assert_eq!(kinds[1].1, Vec2::new(250.0, 250.0 * 9.0 / 16.0));
    }

    #[test]
    fn image_width_percent_uses_column() {
        let out = run(
            "![x](a.png \"width=50%\")
",
            None,
            Some(Vec2::new(100.0, 50.0)),
        );
        let Some(Deco::Image { size, .. }) = out.decos.first() else { panic!() };
        assert_eq!(*size, Vec2::new(250.0, 125.0));
    }
}
