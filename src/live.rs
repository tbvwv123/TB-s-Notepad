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
    /// 목록의 `-` 자리에 점. `text_at` 은 기호 뒤 글이 시작하는 글자 위치
    Bullet { at: usize, level: usize, text_at: usize },
    /// `[ ]` 자리에 체크박스. `byte`는 원문에서 공백/x 가 있는 바이트 위치, `text_at` 은 글이 시작하는 위치
    Task { at: usize, checked: bool, byte: usize, text_at: usize },
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
    /// 토글 제목 줄의 ▶ / ▼. `details_byte` 는 `<details>` 줄의 시작 바이트 (누르면 open 을 바꿈),
    /// `depth` 는 바깥 토글 수 (들여쓰기), `empty` 는 제목이 비었는지
    Toggle { at: usize, open: bool, details_byte: usize, depth: usize, empty: bool },
    /// 커서가 밖에 있는 표: 원문은 숨기고 칸과 선으로 그립니다 (구분 줄 `| --- |` 은 행에 없음)
    Table { rows: Vec<TableRow>, aligns: Vec<ImgAlign>, row_height: f32 },
    /// Ctrl+클릭으로 열 링크
    Link { start: usize, end: usize, url: String },
    /// `#태그`: 둥근 바탕을 깔고, Ctrl+클릭하면 그 태그가 붙은 메모를 모아 봄
    Tag { start: usize, end: usize, tag: String },
    /// 유니코드 이모지 글자 (기본 트위모지가 있는 것): 글자는 투명하게 두고 그 자리에 그림
    Twemoji { start: usize, end: usize, idx: usize, size: f32 },
    /// 글 속 커스텀 이모지 (`![:이름:](주소 "emoji")`). `start`..`end` 는 문법 전체의 글자 범위로
    /// 글자는 숨기고, 첫 글자 뒤에 비워 둔 `size` 크기 자리에 그림을 그립니다.
    Emoji { start: usize, end: usize, uri: String, size: f32 },
}

/// 표의 한 행. `at` 은 그 줄의 첫 글자 위치
#[derive(Clone, Debug, PartialEq)]
pub struct TableRow {
    pub at: usize,
    pub cells: Vec<String>,
    pub header: bool,
}

#[derive(Clone)]
pub struct LiveStyle {
    pub size: f32,
    /// 본문 글꼴의 한 줄 높이. 숨긴 글자만 있는 줄(이모지만 있는 줄, 제목이 빈 토글)이 납작해지지 않게 씀
    pub row_height: f32,
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
    /// `==형광펜==` 바탕색
    pub mark: Color32,
    /// `#태그` 글자색
    pub tag: Color32,
}

impl LiveStyle {
    pub fn hash_into(&self, h: &mut impl Hasher) {
        self.size.to_bits().hash(h);
        self.row_height.to_bits().hash(h);
        self.body.hash(h);
        self.bold.hash(h);
        self.mono.hash(h);
        for c in
            [self.text, self.muted, self.heading, self.code, self.code_bg, self.link, self.quote, self.mark, self.tag]
        {
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

/// 글 속 이모지의 크기 (글자 크기 대비)
pub const EMOJI_SCALE: f32 = 1.25;

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
    /// 문서가 있는 폴더 (이모지 그림 주소를 풀 때)
    base_dir: Option<&'a Path>,
    chars: usize,
    decos: Vec<Deco>,
    /// 이 줄 첫 글자 앞에 둘 빈 칸 (토글 안 들여쓰기)
    lead: f32,
}

impl<'a> Builder<'a> {
    fn push(&mut self, s: &str, f: TextFormat) {
        if !s.is_empty() {
            let lead = std::mem::take(&mut self.lead);
            self.job.append(s, lead, f);
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

    /// 보이지 않지만 줄 높이는 글자 크기 `size` 의 보통 줄만큼 차지하는 글자
    /// (숨긴 글자만 있는 줄도 커서와 줄 간격이 보통 줄과 같도록)
    fn hidden_row(&self, size: f32) -> TextFormat {
        TextFormat { line_height: Some(self.st.row_height * size / self.st.size), ..self.hidden() }
    }

    /// 커서 줄이면 흐리게, 아니면 숨김
    fn marker(&self, active: bool) -> TextFormat {
        if active { self.fmt(&self.st.body, 1.0, self.st.muted) } else { self.hidden() }
    }

    /// [`Self::marker`] 와 같되, 드러낼 때는 곁의 글자 크기 `size` 에 맞춤 (제목 · 작은 글씨의 기호)
    fn marker_sized(&self, active: bool, size: f32) -> TextFormat {
        let mut m = self.marker(active);
        if active {
            m.font_id.size = size;
        }
        m
    }
}

/// 배치할 한 줄
struct Line<'a> {
    /// 줄 번호 (0부터)
    no: usize,
    /// 커서가 있는 줄 (기호를 흐리게 드러냄)
    active: bool,
    /// 이 줄이 원문에서 시작하는 바이트 위치와 글자 위치
    start_byte: usize,
    start_char: usize,
    /// 줄바꿈까지 포함한 줄
    raw: &'a str,
    /// 줄바꿈을 뺀 내용
    body: &'a str,
    /// `"\n"`, 마지막 줄이면 `""`
    nl: &'a str,
    /// 앞 들여쓰기를 뺀 내용과 들여쓰기 바이트 수
    trimmed: &'a str,
    indent: usize,
}

pub fn layout(input: &LiveInput<'_>) -> LiveLayout {
    let text = input.text;
    let mut b = Builder {
        job: LayoutJob::default(),
        st: input.style,
        base_dir: input.base_dir,
        chars: 0,
        decos: Vec::new(),
        lead: 0.0,
    };
    let mut image_uris = Vec::new();

    if text.len() > MAX_LIVE_BYTES {
        let f = b.body();
        b.push(text, f);
        return LiveLayout { job: b.job, decos: b.decos, image_uris };
    }

    let tables = find_tables(text);
    let toggles = find_toggles(text);
    let mut table_rows: Vec<TableRow> = Vec::new();
    let mut fence: Option<(&str, usize)> = None; // (여는 기호, 블록 시작 글자 위치)
    for (no, raw) in text.split_inclusive('\n').enumerate() {
        let (body, nl) = match raw.strip_suffix('\n') {
            Some(x) => (x, "\n"),
            None => (raw, ""),
        };
        let trimmed = body.trim_start();
        let line = Line {
            no,
            active: input.active_line == Some(no),
            start_byte: raw.as_ptr() as usize - text.as_ptr() as usize,
            start_char: b.chars,
            raw,
            body,
            nl,
            trimmed,
            indent: body.len() - trimmed.len(),
        };
        b.lead = 0.0;
        // 앞선 것이 먼저: 코드 블록 안에서는 토글 · 표 · 그림 문법을 보지 않습니다.
        let done = (fence.is_none() && b.toggle_line(&line, &toggles, input.active_line))
            || b.fence_line(&line, &mut fence)
            || b.table_line(&line, &tables, input.active_line, &mut table_rows)
            || b.media_line(&line, input, &mut image_uris)
            || b.subtext_line(&line)
            || b.heading_line(&line)
            || b.rule_line(&line);
        if !done {
            b.text_line(&line);
        }
    }
    if let Some((_, start)) = fence {
        let end = b.chars;
        b.decos.push(Deco::CodeBlock { start, end });
    }
    // 글이 줄바꿈으로 끝나면 맨 끝의 빈 줄은 글자가 없어서, egui 가 앞 줄 줄바꿈의 서식에서 높이를 가져옵니다.
    // 그러면 그림 뒤에서는 그림만큼 높고, 제목 뒤에서는 제목만큼 크고, 닫는 ``` · </details> 뒤에서는 납작해져
    // 커서가 이상해지므로, 빈 구간을 덧붙여 보통 줄 높이로 정합니다.
    if text.ends_with('\n') {
        let f = b.body();
        b.job.append("", 0.0, f);
    }
    LiveLayout { job: b.job, decos: b.decos, image_uris }
}

/// `#` 개수에 따른 제목 글자 크기 (본문 대비)
fn heading_scale(level: usize) -> f32 {
    match level {
        1 => 1.75,
        2 => 1.45,
        3 => 1.22,
        _ => 1.08,
    }
}

/// 줄 종류마다 하나씩. `bool` 을 돌려주는 것은 그 종류의 줄이어서 배치를 마쳤으면 `true`.
impl Builder<'_> {
    /// 토글: 접힌 내용은 숨기고, `<details>` · `</details>` 줄은 커서가 있을 때만 흐리게, 제목 줄은 ▶ 와 함께.
    /// 토글 줄이 아니면 펼친 토글 안에 있는 만큼 들여쓰기만 정합니다.
    fn toggle_line(&mut self, l: &Line<'_>, toggles: &[ToggleBlock], active_line: Option<usize>) -> bool {
        let st = self.st;
        let indent_w = st.size * TOGGLE_INDENT_SCALE;
        if toggles.iter().any(|t| !t.shown_open(active_line) && l.no > t.summary && l.no <= t.close) {
            let f = self.hidden();
            self.push(l.raw, f);
            return true;
        }
        if toggles.iter().any(|t| l.no == t.details || l.no == t.close) {
            let f = if l.active { self.fmt(&st.mono, 0.85, st.muted) } else { self.hidden() };
            self.push(l.raw, f);
            return true;
        }
        let Some(t) = toggles.iter().find(|t| l.no == t.summary) else {
            let depth = toggles.iter().filter(|t| l.no > t.summary && l.no < t.close).count();
            self.lead = depth as f32 * indent_w;
            return false;
        };
        let title = crate::editing::summary_line(l.body).unwrap_or(0..0);
        self.lead = (t.depth + 1) as f32 * indent_w;
        // <summary> 태그는 커서가 있어도 숨김 (커서는 제목 안에만 머묾).
        // 제목이 비어도 줄이 납작해지지 않게 보통 줄 높이를 줌
        let tag = self.hidden_row(st.size);
        self.push(&l.body[..title.start], tag.clone());
        // 제목 앞의 `# ` · `## ` · `### ` 는 제목 크기 토글 (기호는 커서가 있을 때만 흐리게)
        let tt = &l.body[title.clone()];
        let hashes = tt.bytes().take_while(|&c| c == b'#').count();
        if (1..=3).contains(&hashes) && tt[hashes..].starts_with(' ') {
            let scale = heading_scale(hashes);
            let mf = self.marker_sized(l.active, st.size * scale);
            self.push(&tt[..hashes + 1], mf);
            let hf = self.fmt(&st.bold, scale, st.heading);
            inline(self, &tt[hashes + 1..], &hf, l.active);
        } else {
            let base = self.body();
            inline(self, tt, &base, l.active);
        }
        self.push(&l.body[title.end..], tag);
        let nl_f = self.body();
        self.push(l.nl, nl_f);
        self.decos.push(Deco::Toggle {
            at: l.start_char,
            open: t.shown_open(active_line),
            details_byte: t.details_byte,
            depth: t.depth,
            empty: title.is_empty(),
        });
        true
    }

    /// ``` 코드 블록: 여닫는 줄은 커서가 있을 때만 흐리게, 안쪽은 고정폭으로.
    /// `fence`: 열려 있는 블록의 (여는 기호, 시작 글자 위치)
    fn fence_line(&mut self, l: &Line<'_>, fence: &mut Option<(&'static str, usize)>) -> bool {
        let st = self.st;
        let is_fence = ["```", "~~~"].into_iter().find(|f| l.trimmed.starts_with(f));
        let edge = |b: &Self| if l.active { b.fmt(&st.mono, 0.85, st.muted) } else { b.hidden() };
        match *fence {
            Some((open, start)) if is_fence == Some(open) => {
                let f = edge(self);
                self.push(l.body, f.clone());
                self.decos.push(Deco::CodeBlock { start, end: self.chars });
                *fence = None;
                self.push(l.nl, f);
            }
            Some(_) => {
                let f = self.fmt(&st.mono, 0.9, st.code);
                self.push(l.raw, f);
            }
            None => {
                let Some(open) = is_fence else { return false };
                *fence = Some((open, l.start_char));
                let f = edge(self);
                self.push(l.raw, f);
            }
        }
        true
    }

    /// 표: 커서가 표 안에 있으면 고칠 수 있게 원문(고정폭)으로, 밖에 있으면 원문을 숨기고 칸과 선으로 그림.
    /// `rows`: 이 표에서 지금까지 모은 행
    fn table_line(
        &mut self,
        l: &Line<'_>,
        tables: &[TableBlock],
        active_line: Option<usize>,
        rows: &mut Vec<TableRow>,
    ) -> bool {
        let st = self.st;
        let Some(tb) = tables.iter().find(|t| (t.start..t.end).contains(&l.no)) else { return false };
        let separator = l.no == tb.start + 1;
        if active_line.is_some_and(|a| (tb.start..tb.end).contains(&a)) {
            let f = self.fmt(&st.mono, 0.9, if separator { st.muted } else { st.text });
            self.push(l.body, f.clone());
            self.push(l.nl, f);
            return true;
        }
        let row_height = (st.size * TABLE_ROW_SCALE).round();
        let mut f = self.hidden();
        f.line_height = Some(if separator { 1.0 } else { row_height });
        if !separator {
            let cells = split_cells(l.body).iter().map(|c| plain_cell(c)).collect();
            rows.push(TableRow { at: l.start_char, cells, header: l.no == tb.start });
        }
        self.push(l.body, f.clone());
        self.push(l.nl, f);
        if l.no + 1 == tb.end {
            let rows = std::mem::take(rows);
            self.decos.push(Deco::Table { rows, aligns: tb.aligns.clone(), row_height });
        }
        true
    }

    /// 그림·소리·영상만 있는 줄: 경로 글자는 커서가 있어도 숨기고 그림(카드)만 보여 줍니다
    /// (크기·위치·삭제는 그림을 눌러서 나오는 도구로). 아직 못 불러온 그림은 보통 줄(링크 모양)로 둡니다.
    fn media_line(&mut self, l: &Line<'_>, input: &LiveInput<'_>, image_uris: &mut Vec<String>) -> bool {
        let Some(img) = parse_image_line(l.trimmed) else { return false };
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
        let Some(size) = size else { return false };
        let top = 6.0;
        let mut f = self.hidden();
        f.line_height = Some(top + size.y + 10.0);
        self.push(l.body, f.clone());
        self.push(l.nl, f);
        self.decos.push(Deco::Image {
            kind,
            at: l.start_char,
            end: l.start_char + l.body.chars().count(),
            uri,
            size,
            top,
            align: img.align,
            line: l.start_byte..l.start_byte + l.body.len(),
            max_w,
        });
        true
    }

    /// `-# 작은 글씨` (디스코드식 보조 글): 기호는 숨기고 작고 흐리게
    fn subtext_line(&mut self, l: &Line<'_>) -> bool {
        let st = self.st;
        let Some(rest) = subtext_body(l.trimmed) else { return false };
        let marker_end = l.body.len() - rest.len();
        let mf = self.marker_sized(l.active, st.size * SUBTEXT_SCALE);
        self.push(&l.body[..marker_end], mf);
        let sf = self.fmt(&st.body, SUBTEXT_SCALE, st.muted);
        inline(self, rest, &sf, l.active);
        self.push(l.nl, sf);
        true
    }

    /// `# 제목`
    fn heading_line(&mut self, l: &Line<'_>) -> bool {
        let st = self.st;
        let hashes = l.trimmed.bytes().take_while(|&c| c == b'#').count();
        if !(1..=6).contains(&hashes) || l.trimmed[hashes..].chars().next().is_some_and(|c| c != ' ') {
            return false;
        }
        let scale = heading_scale(hashes);
        let marker_end = (l.indent + hashes + 1).min(l.body.len());
        let mf = self.marker_sized(l.active, st.size * scale);
        self.push(&l.body[..marker_end], mf);
        let hf = self.fmt(&st.bold, scale, st.heading);
        inline(self, &l.body[marker_end..], &hf, l.active);
        self.push(l.nl, hf);
        true
    }

    /// `---` 구분선: 커서가 있으면 글자 그대로, 없으면 선
    fn rule_line(&mut self, l: &Line<'_>) -> bool {
        let st = self.st;
        let compact: String = l.trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.len() < 3 || !['-', '*', '_'].iter().any(|&m| compact.chars().all(|c| c == m)) {
            return false;
        }
        if l.active {
            let f = self.fmt(&st.body, 1.0, st.muted);
            self.push(l.raw, f);
        } else {
            let mut f = self.hidden();
            f.line_height = Some(st.size * 1.4);
            self.push(l.body, f.clone());
            self.push(l.nl, f);
            self.decos.push(Deco::Rule { at: l.start_char });
        }
        true
    }

    /// 그 밖의 줄: 인용 · 목록 · (표가 아닌) `|` 줄의 앞머리를 처리하고 나머지는 줄 안 서식으로
    fn text_line(&mut self, l: &Line<'_>) {
        let st = self.st;
        let mut rest = l.indent;
        let mut base = self.body();
        self.push(&l.body[..l.indent], base.clone());

        if l.trimmed.starts_with('>') {
            let n = l.trimmed.bytes().take_while(|&c| c == b'>' || c == b' ').count();
            let f = self.marker(l.active);
            self.push(&l.body[rest..rest + n], f);
            rest += n;
            base = self.fmt(&st.body, 1.0, st.quote);
            let end = l.start_char + l.body.chars().count();
            self.decos.push(Deco::Quote { start: l.start_char, end });
        } else if let Some(m) = list_marker(l.trimmed) {
            if let Some(checked) = self.list_prefix(l, &m, &mut rest) {
                base = checked;
            }
        } else if l.trimmed.starts_with('|') {
            // 구분 줄이 없어 표가 아닌 `|` 줄: 고정폭으로 줄 맞춤
            base = self.fmt(&st.mono, 0.9, st.text);
            let pipe = self.fmt(&st.mono, 0.9, st.muted);
            for (i, cell) in l.body[rest..].split('|').enumerate() {
                if i > 0 {
                    self.push("|", pipe.clone());
                }
                self.push(cell, base.clone());
            }
            self.push(l.nl, base);
            return;
        }

        inline(self, &l.body[rest..], &base, l.active);
        self.push(l.nl, base);
    }

    /// 목록 기호 (`- ` · `1. ` · `- [ ] `) 를 넣고 `rest` 를 그 뒤로 옮깁니다.
    /// 끝낸 할 일이면 나머지 글에 쓸 서식(흐리게 · 취소선)을 돌려줍니다.
    fn list_prefix(&mut self, l: &Line<'_>, m: &ListMarker, rest: &mut usize) -> Option<TextFormat> {
        let st = self.st;
        let body = l.body;
        let after = &body[*rest + m.len..];
        let task = ["[ ] ", "[x] ", "[X] "].into_iter().find(|t| after.starts_with(t) || after == t.trim_end());
        match (&m.kind, task) {
            (ListKind::Bullet, Some(t)) => {
                // "- " 는 숨기고 "[ ]" 자리에 체크박스
                let f = self.hidden();
                self.push(&body[*rest..*rest + m.len], f);
                *rest += m.len;
                let checked = t != "[ ] ";
                let at = self.chars;
                let box_len = 3.min(body.len() - *rest);
                let mut bf = self.body();
                bf.color = Color32::TRANSPARENT;
                self.push(&body[*rest..*rest + box_len], bf);
                let space = usize::from(body[*rest + box_len..].starts_with(' '));
                let text_at = at + box_len + space;
                self.decos.push(Deco::Task { at, checked, byte: l.start_byte + *rest + 1, text_at });
                *rest += box_len;
                checked.then(|| {
                    let mut done = self.fmt(&st.body, 1.0, st.muted);
                    done.strikethrough = Stroke::new(1.0, st.muted);
                    done
                })
            }
            (ListKind::Bullet, None) => {
                let at = self.chars;
                let mut f = self.body();
                f.color = Color32::TRANSPARENT;
                self.push(&body[*rest..*rest + m.len], f);
                *rest += m.len;
                self.decos.push(Deco::Bullet { at, level: l.indent / 2, text_at: self.chars });
                None
            }
            (ListKind::Number, _) => {
                let f = self.fmt(&st.body, 1.0, st.muted);
                self.push(&body[*rest..*rest + m.len], f);
                *rest += m.len;
                None
            }
        }
    }
}

enum ListKind {
    Bullet,
    Number,
}

struct ListMarker {
    kind: ListKind,
    len: usize,
}

/// 토글 들여쓰기 한 단계 (글자 크기 대비). 꾸밈 그리기에서도 씀
pub const TOGGLE_INDENT_SCALE: f32 = 1.4;

/// 토글 한 덩어리 (줄 번호): `<details>` · `<summary>` · `</details>`
/// 토글 하나 (줄 번호는 0부터)
pub(crate) struct ToggleBlock {
    pub details: usize,
    pub summary: usize,
    pub close: usize,
    pub open: bool,
    /// 바깥 토글 수
    pub depth: usize,
    pub details_byte: usize,
}

impl ToggleBlock {
    /// 펼쳐 보일지: 접힌 토글이라도 커서가 그 안에 들어가면 펼쳐 보여 줌
    fn shown_open(&self, active_line: Option<usize>) -> bool {
        self.open || active_line.is_some_and(|a| a > self.summary && a < self.close)
    }
}

/// 문서의 토글들 (코드 블록 안은 빼고, 닫히지 않은 것도 뺌)
pub(crate) fn find_toggles(text: &str) -> Vec<ToggleBlock> {
    use crate::editing::{is_toggle_close, summary_line, toggle_open_line};
    let mut lines = Vec::new();
    let mut at = 0;
    for l in text.split('\n') {
        lines.push((at, l.strip_suffix('\r').unwrap_or(l)));
        at += l.len() + 1;
    }
    let mut out = Vec::new();
    let mut stack: Vec<(usize, bool, usize)> = Vec::new(); // (<details> 줄, open, 바이트)
    let mut in_fence = false;
    let mut i = 0;
    while i < lines.len() {
        let (byte, l) = lines[i];
        let t = l.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        } else if !in_fence {
            if let Some(open) = toggle_open_line(l)
                && lines.get(i + 1).is_some_and(|(_, next)| summary_line(next).is_some())
            {
                stack.push((i, open, byte));
                i += 2;
                continue;
            }
            if is_toggle_close(l)
                && let Some((details, open, details_byte)) = stack.pop()
            {
                out.push(ToggleBlock {
                    details,
                    summary: details + 1,
                    close: i,
                    open,
                    depth: stack.len(),
                    details_byte,
                });
            }
        }
        i += 1;
    }
    out
}

/// 표 한 행의 높이 (글자 크기 대비)
const TABLE_ROW_SCALE: f32 = 1.9;

/// 표 한 덩어리: 줄 번호 `start..end` (머리 줄, 구분 줄, 내용 줄들)
struct TableBlock {
    start: usize,
    end: usize,
    aligns: Vec<ImgAlign>,
}

/// `| 가 | 나 |` → `["가", "나"]`. `\|` 와 `` `코드` `` 안의 `|` 는 칸을 나누지 않습니다.
pub fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = if t.ends_with('|') && !t.ends_with("\\|") { &t[..t.len() - 1] } else { t };
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut code = false;
    let mut chars = t.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '`' => {
                code = !code;
                cur.push(ch);
            }
            '|' if !code => cells.push(std::mem::take(&mut cur).trim().to_owned()),
            _ => cur.push(ch),
        }
    }
    cells.push(cur.trim().to_owned());
    cells
}

/// `| --- | :---: | ---: |` 같은 구분 줄이면 칸마다 정렬
fn table_separator(line: &str) -> Option<Vec<ImgAlign>> {
    if !line.contains('|') {
        return None;
    }
    split_cells(line)
        .iter()
        .map(|c| {
            let inner = c.strip_prefix(':').unwrap_or(c);
            let inner = inner.strip_suffix(':').unwrap_or(inner);
            if inner.is_empty() || !inner.chars().all(|ch| ch == '-') {
                return None;
            }
            Some(match (c.starts_with(':'), c.ends_with(':')) {
                (true, true) => ImgAlign::Center,
                (false, true) => ImgAlign::Right,
                _ => ImgAlign::Left,
            })
        })
        .collect()
}

/// 문서의 표들 (코드 블록 안은 빼고)
fn find_tables(text: &str) -> Vec<TableBlock> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            i += 1;
            continue;
        }
        if !in_fence
            && lines[i].contains('|')
            && let Some(aligns) = lines.get(i + 1).and_then(|l| table_separator(l))
            && split_cells(lines[i]).len() == aligns.len()
        {
            let mut end = i + 2;
            while end < lines.len() && lines[end].contains('|') && !lines[end].trim().is_empty() {
                end += 1;
            }
            out.push(TableBlock { start: i, end, aligns });
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

/// 칸 안의 글자에서 마크다운 기호를 뺀 모양 (`**굵게**` → `굵게`, `[글](주소)` → `글`)
fn plain_cell(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len());
    let mut rest = cell;
    while let Some(ch) = rest.chars().next() {
        if ch == '!' && rest[1..].starts_with('[') {
            rest = &rest[1..];
            continue;
        }
        if ch == '['
            && let Some(close) = rest.find("](")
            && let Some(paren) = rest[close..].find(')')
        {
            out.push_str(&rest[1..close]);
            rest = &rest[close + paren + 1..];
            continue;
        }
        if let Some(r) = rest.strip_prefix("<br>").or_else(|| rest.strip_prefix("<br/>")) {
            out.push(' ');
            rest = r;
            continue;
        }
        if let Some(r) = ["**", "__", "~~"].iter().find_map(|m| rest.strip_prefix(m)) {
            rest = r;
            continue;
        }
        if ch != '`' {
            out.push(ch);
        }
        rest = &rest[ch.len_utf8()..];
    }
    out
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
    // 제목은 따옴표 한 쌍뿐 (`![a](x "t") 글 ![b](y "t")` 처럼 그림 여럿이 있는 줄은 그림 줄이 아님)
    let title_ok =
        after.len() >= 2 && after.starts_with('"') && after.ends_with('"') && !after[1..after.len() - 1].contains('"');
    if dest.is_empty() || (!after.is_empty() && !title_ok) {
        return None;
    }
    let mut width = None;
    let mut align = ImgAlign::Left;
    for token in after.trim_matches('"').split_whitespace() {
        match token.split_once('=') {
            Some(("width", v)) => width = v.trim_end_matches('%').parse::<u32>().ok().map(|w| w.clamp(5, 100)),
            Some(("align", "center")) => align = ImgAlign::Center,
            Some(("align", "right")) => align = ImgAlign::Right,
            None if token == crate::emoji::TITLE => return None,
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
    let marker = b.marker_sized(active, base.font_id.size);
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
        for (m, kind) in [("**", 0), ("__", 0), ("~~", 1), ("==", 2)] {
            if rest.starts_with(m)
                && let Some(close) = rest[2..].find(m)
                && close > 0
                // ==형광펜== 은 안쪽 양 끝이 띄어쓰기가 아니어야 함 (`a == b` 같은 글은 그대로)
                && (kind != 2 || !(rest[2..].starts_with(' ') || rest[2..2 + close].ends_with(' ')))
            {
                flush(b, plain, i);
                b.push(m, marker.clone());
                let mut f = base.clone();
                match kind {
                    0 => f.font_id.family = st.bold.clone(),
                    1 => f.strikethrough = Stroke::new(1.0, base.color),
                    _ => f.background = st.mark,
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

        // #태그 (줄 맨 앞이나 띄어쓰기 뒤)
        if c == b'#'
            && s[..i].chars().next_back().is_none_or(char::is_whitespace)
            && let Some((tag, len)) = crate::tags::tag_at(rest)
        {
            flush(b, plain, i);
            let mut f = base.clone();
            f.color = st.tag;
            let start = b.chars;
            b.push(&rest[..len], f);
            b.decos.push(Deco::Tag { start, end: b.chars, tag: tag.to_lowercase() });
            i += len;
            plain = i;
            continue;
        }

        // 커스텀 이모지: 문법은 늘 숨기고 그 자리에 글자만 한 그림을 그림 (decorations.rs)
        if c == b'!'
            && let Some(e) = crate::emoji::parse_inline(rest)
        {
            flush(b, plain, i);
            let size = base.font_id.size * EMOJI_SCALE;
            let start = b.chars;
            // 이모지만 있는 줄도 보통 줄 높이 (숨긴 글자뿐이면 줄이 납작해져 커서·줄 번호가 겹침)
            let hidden = b.hidden_row(base.font_id.size);
            // 첫 글자는 그림 앞(커서가 그림 왼쪽에 서도록), 나머지 앞에 그림 자리를 비워 둠
            b.push(&rest[..1], hidden.clone());
            b.lead = size + 4.0;
            b.push(&rest[1..e.len], hidden);
            let uri = resolve_uri(e.dest, b.base_dir);
            b.decos.push(Deco::Emoji { start, end: b.chars, uri, size });
            i += e.len;
            plain = i;
            continue;
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

        // 기본 이모지 글자 (U+2000~ 기호는 0xE2, 그림 이모지는 0xF0 으로 시작)
        if (c == 0xE2 || c == 0xF0)
            && !crate::twemoji::joined_after(&s[..i])
            && let Some((idx, len)) = crate::twemoji::at(rest)
        {
            flush(b, plain, i);
            let size = base.font_id.size * EMOJI_SCALE * 0.95;
            // 글자는 숨기고 (글꼴마다 이모지 폭이 달라서) 바로 다음 글자 앞에 그림 폭만큼 빈 자리를 둠.
            // 커서는 이모지 앞(그림 왼쪽)과 다음 글자 앞(그림 오른쪽)에 섬. 줄 높이는 보통 줄 그대로
            let start = b.chars;
            let mut f = b.hidden_row(base.font_id.size);
            if i + len >= bytes.len() {
                // 맨 끝이면 뒤 글자가 없어 빈 자리를 둘 수 없으므로, 투명한 글자를 크게 해서 자리를 냄
                // (그래야 줄 끝의 커서가 그림 오른쪽에 섬)
                f.font_id.size = base.font_id.size * 1.3;
                b.push(&rest[..len], f);
            } else {
                b.push(&rest[..len], f);
                b.lead = size + 3.0;
            }
            b.decos.push(Deco::Twemoji { start, end: b.chars, idx, size });
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
            row_height: 21.0,
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
            mark: Color32::YELLOW,
            tag: Color32::GREEN,
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
                Deco::Task { at, checked, byte, .. } => {
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

    /// 배치한 뒤 각 행의 높이
    fn row_heights(text: &str, img: Option<Vec2>) -> Vec<f32> {
        let job = run(text, None, img).job;
        let ctx = egui::Context::default();
        let mut heights = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let galley = ui.fonts_mut(|f| f.layout_job(job.clone()));
            heights = galley.rows.iter().map(|r| r.height()).collect();
        });
        // 테스트에서는 글꼴 텍스처를 쓰지 않으므로 버림
        out.textures_delta.clear();
        heights
    }

    #[test]
    fn last_empty_line_is_a_normal_row() {
        let normal = row_heights("글\n", None)[1];
        let img = Some(Vec2::new(400.0, 300.0));
        // 그림을 붙여넣은 뒤 · 그림 뒤에서 Enter 를 친 뒤의 맨 끝 빈 줄 (커서가 서는 줄)
        let rows = row_heights("글\n![그림](a.png)\n", img);
        assert!(rows[1] > 300.0, "그림 줄은 그림 높이: {rows:?}");
        assert_eq!(rows[2], normal, "그림 아래 빈 줄은 보통 줄 높이: {rows:?}");
        // 제목 · 표 · 닫는 코드 블록 · 닫는 토글 줄 뒤도 마찬가지 (크거나 납작해지지 않음)
        for text in [
            "# 제목\n",
            "| a | b |\n| - | - |\n| 1 | 2 |\n",
            "```\n코드\n```\n",
            "<details open>\n<summary>토글</summary>\n\n내용\n\n</details>\n",
            "---\n",
        ] {
            let rows = row_heights(text, None);
            assert_eq!(rows.last(), Some(&normal), "{text:?}: {rows:?}");
        }
        // 가운데 줄들은 그대로: 그림 줄만 높고 그 아래 빈 줄 · 글 줄은 보통 높이
        let rows = row_heights("![그림](a.png)\n\n글\n", img);
        assert!(rows[0] > 300.0 && rows[1..].iter().all(|h| *h == normal), "{rows:?}");
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
    fn inline_emoji_keeps_text_and_reserves_space() {
        let src = "오늘 ![:party:](.emoji/party.png \"emoji\") 끝\n![:cat:](.emoji/cat.png \"emoji\")\n";
        for active in [None, Some(0), Some(1)] {
            let out = run(src, active, None);
            assert_eq!(out.job.text, src, "글자는 그대로");
            let emoji: Vec<_> = out
                .decos
                .iter()
                .filter_map(|d| match d {
                    Deco::Emoji { start, end, uri, .. } => Some((*start, *end, uri.clone())),
                    _ => None,
                })
                .collect();
            assert_eq!(emoji.len(), 2, "이모지만 있는 줄도 큰 그림이 아니라 이모지: {:?}", out.decos);
            let chars: Vec<char> = src.chars().collect();
            let (s, e, uri) = &emoji[0];
            assert_eq!(chars[*s..*e].iter().collect::<String>(), "![:party:](.emoji/party.png \"emoji\")");
            assert!(uri.ends_with("party.png"), "{uri}");
            assert!(!out.decos.iter().any(|d| matches!(d, Deco::Image { .. } | Deco::Link { .. })));
            // 그림 자리는 두 번째 글자 앞에 비워 둠
            assert!(out.job.sections.iter().any(|sec| sec.leading_space > 0.0));
            // 숨긴 문법도 보통 줄 높이를 차지 (이모지만 있는 줄이 납작해지지 않게)
            let first = out.job.sections.iter().find(|sec| sec.byte_range.start.0 == "오늘 ".len()).unwrap();
            assert_eq!(first.format.line_height, Some(21.0));
        }
        assert!(parse_image_line("![:cat:](cat.png \"emoji\")").is_none());
        assert!(parse_image_line("![:a:](a.png \"emoji\") 글 ![:b:](b.png \"emoji\")").is_none(), "이모지 여럿인 줄");
        assert!(parse_image_line("![a](a.png \"width=50%\") 글 ![b](b.png \"x\")").is_none());
        assert_eq!(plain_cell("![:cat:](cat.png \"emoji\") 고양이"), ":cat: 고양이");
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
    fn highlights_and_tags() {
        let src = "오늘 ==중요한 일== 과 a == b, #회의 #일/2024 그리고 C#\n## 제목 #태그\n";
        let out = run(src, None, None);
        assert_eq!(out.job.text, src);
        let marked: Vec<&str> = out
            .job
            .sections
            .iter()
            .filter(|sec| sec.format.background == Color32::YELLOW)
            .map(|sec| &src[sec.byte_range.start.0..sec.byte_range.end.0])
            .collect();
        assert_eq!(marked, ["중요한 일"], "a == b 는 형광펜이 아님");
        let tags: Vec<&str> = out
            .decos
            .iter()
            .filter_map(|d| match d {
                Deco::Tag { tag, .. } => Some(tag.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(tags, ["회의", "일/2024", "태그"]);
    }

    #[test]
    fn toggle_titles_take_headings_and_emoji() {
        let src = "<details open>\n<summary>## 회의 ![:cat:](cat.png \"emoji\") #일</summary>\n\n내용\n\n</details>\n";
        let out = run(src, None, None);
        assert_eq!(out.job.text, src);
        let big = out.job.sections.iter().any(|sec| {
            sec.format.font_id.size > 20.0 && src[sec.byte_range.start.0..sec.byte_range.end.0].contains("회의")
        });
        assert!(big, "## 는 제목 크기");
        assert!(out.decos.iter().any(|d| matches!(d, Deco::Emoji { .. })));
        assert!(out.decos.iter().any(|d| matches!(d, Deco::Tag { .. })));
    }

    #[test]
    fn unicode_emoji_become_twemoji() {
        let src = "좋아요 👍 그리고 ❤️ 끝 `😄`\n";
        let out = run(src, None, None);
        assert_eq!(out.job.text, src);
        let n = out.decos.iter().filter(|d| matches!(d, Deco::Twemoji { .. })).count();
        assert_eq!(n, 2, "코드 안의 것은 그대로");
        // 피부색 · 가족 이모지는 통째로 글꼴에 맡김 (일부만 그림이 되지 않게)
        let out = run("👍🏽 👨\u{200d}👩\u{200d}👧\n", None, None);
        assert!(!out.decos.iter().any(|d| matches!(d, Deco::Twemoji { .. })), "{:?}", out.decos);
    }

    #[test]
    fn empty_toggle_title_keeps_its_row() {
        let src = "<details open>\n<summary></summary>\n\n\n\n</details>\n";
        let out = run(src, None, None);
        let summary = "<details open>\n".len();
        let sec = out.job.sections.iter().find(|sec| sec.byte_range.start.0 == summary).unwrap();
        assert_eq!(sec.format.line_height, Some(21.0), "제목이 빈 토글 줄도 보통 높이");
        assert!(out.decos.iter().any(|d| matches!(d, Deco::Toggle { empty: true, .. })));
    }

    #[test]
    fn toggles_fold_their_content() {
        let text = "앞\n<details>\n<summary>회의 메모</summary>\n\n- 안건\n\n</details>\n뒤";
        let out = run(text, None, None);
        assert_eq!(out.job.text, text);
        let toggle = out.decos.iter().find_map(|d| match d {
            Deco::Toggle { open, details_byte, depth, empty, .. } => Some((*open, *details_byte, *depth, *empty)),
            _ => None,
        });
        assert_eq!(toggle, Some((false, 4, 0, false)));
        assert!(!out.decos.iter().any(|d| matches!(d, Deco::Bullet { .. })), "접힌 안쪽 목록은 숨김");
        // 펼치면 안쪽이 보이고, 커서가 안에 있으면 접혀 있어도 펼쳐 보여 줌
        let open = run(&text.replace("<details>", "<details open>"), None, None);
        assert!(open.decos.iter().any(|d| matches!(d, Deco::Bullet { .. })));
        let inside = run(text, Some(4), None);
        assert!(inside.decos.iter().any(|d| matches!(d, Deco::Toggle { open: true, .. })));
        // 코드 블록 안의 <details> 는 토글이 아님
        let code = run("```\n<details>\n<summary>x</summary>\n</details>\n```\n", None, None);
        assert!(!code.decos.iter().any(|d| matches!(d, Deco::Toggle { .. })));
    }

    #[test]
    fn tables_render_as_grid_until_edited() {
        let text = "앞\n| 기능 | 단축키 |\n| :--- | ---: |\n| **찾기** | `Ctrl+F` |\n| 바꾸기 | Ctrl\\|H |\n\n뒤";
        let out = run(text, None, None);
        assert_eq!(out.job.text, text, "글자는 그대로");
        let Some(Deco::Table { rows, aligns, .. }) = out.decos.iter().find(|d| matches!(d, Deco::Table { .. })) else {
            panic!("{:?}", out.decos)
        };
        assert_eq!(aligns, &[ImgAlign::Left, ImgAlign::Right]);
        assert_eq!(rows.len(), 3, "구분 줄은 행이 아님");
        assert!(rows[0].header && !rows[1].header);
        assert_eq!(rows[0].at, 2);
        assert_eq!(rows[1].cells, ["찾기", "Ctrl+F"]);
        assert_eq!(rows[2].cells, ["바꾸기", "Ctrl|H"]);
        // 커서가 표 안(구분 줄)에 있으면 원문 그대로
        let editing = run(text, Some(2), None);
        assert!(!editing.decos.iter().any(|d| matches!(d, Deco::Table { .. })));
        // 코드 블록 안의 표 모양은 표가 아님, 구분 줄 없는 | 도 표가 아님
        let code = run("```\n| a | b |\n| - | - |\n```\na | b\n", None, None);
        assert!(!code.decos.iter().any(|d| matches!(d, Deco::Table { .. })));
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
