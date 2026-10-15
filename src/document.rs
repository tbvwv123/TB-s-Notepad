//! 문서 하나(탭 하나)의 상태와 파일 입출력.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// 한국어 완성형/확장 완성형 (CP949)
    EucKr,
}

impl TextEncoding {
    pub const ALL: [TextEncoding; 5] =
        [TextEncoding::Utf8, TextEncoding::Utf8Bom, TextEncoding::Utf16Le, TextEncoding::Utf16Be, TextEncoding::EucKr];

    pub fn label(self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Utf8Bom => "UTF-8 (BOM)",
            TextEncoding::Utf16Le => "UTF-16 LE",
            TextEncoding::Utf16Be => "UTF-16 BE",
            TextEncoding::EucKr => "ANSI (CP949)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF (Unix)",
            LineEnding::CrLf => "CRLF (Windows)",
        }
    }

    pub fn platform_default() -> Self {
        if cfg!(windows) { LineEnding::CrLf } else { LineEnding::Lf }
    }
}

/// 디스크에 있는 파일의 수정 시각과 크기. 둘 중 하나라도 달라지면 내용을 다시 확인합니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl DiskStamp {
    /// 파일이 없으면 `None`
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self { modified: meta.modified().ok(), len: meta.len() })
    }
}

/// 디스크에서 읽은 파일. 수정 시각을 내용보다 먼저 읽어서, 그사이 파일이 또 바뀌면
/// 다음 확인 때 다시 잡힙니다.
struct DiskSnapshot {
    stamp: Option<DiskStamp>,
    bytes: Vec<u8>,
}

impl DiskSnapshot {
    fn read(path: &Path) -> std::io::Result<Self> {
        let stamp = DiskStamp::of(path);
        let bytes = std::fs::read(path)?;
        Ok(Self { stamp, bytes })
    }
}

/// [`Document::check_disk`]의 결과
#[derive(Debug, PartialEq, Eq)]
pub enum DiskEvent {
    /// 바뀐 것 없음 (또는 이미 충돌 상태)
    Unchanged,
    /// 앱에서 고친 내용이 없어서 조용히 다시 읽었습니다. 커서를 옮기려고 예전 내용을 돌려줍니다.
    Reloaded { old_text: String },
    /// 앱에서도 고친 내용이 있어서 자동 저장을 멈췄습니다. 사용자가 골라야 합니다.
    Conflict,
}

pub struct Document {
    pub id: u64,
    pub path: Option<PathBuf>,
    /// 편집 중인 텍스트. 줄바꿈은 항상 `\n`으로 정규화되어 있습니다.
    pub text: String,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub dirty: bool,
    /// 제목 없는 문서의 번호 ("제목 없음 3")
    pub untitled_index: usize,
    /// 원본 보기 강조 결과 캐시: (키 해시, 레이아웃 잡)
    pub highlight_cache: Option<(u64, egui::text::LayoutJob)>,
    /// 문서 모드 배치 결과 캐시: (키 해시, 레이아웃 잡, 꾸밈, 그림 주소)
    pub live_cache: Option<(u64, egui::text::LayoutJob, Vec<crate::live::Deco>, Vec<String>)>,
    /// 마지막으로 고친 때 (자동 저장용)
    pub last_edit: Option<std::time::Instant>,
    /// "새 메모"로 만든 파일: 첫 줄이 바뀌면 파일 이름도 따라 바뀝니다.
    pub auto_named: bool,
    /// 마지막으로 읽거나 저장했을 때의 파일 수정 시각/크기 (밖에서 바뀐 것을 알아채는 데 씀)
    pub disk: Option<DiskStamp>,
    /// 그때의 파일 내용 해시 (모르면 `None`). 시각만 바뀌고 내용이 같으면 무시하려고.
    disk_hash: Option<u64>,
    /// 앱에서 고친 사이에 파일이 밖에서도 바뀜: 사용자가 고를 때까지 자동 저장을 멈춥니다.
    pub conflict: bool,
}

impl Document {
    pub fn new_untitled(untitled_index: usize) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            path: None,
            text: String::new(),
            encoding: TextEncoding::Utf8,
            line_ending: LineEnding::platform_default(),
            dirty: false,
            untitled_index,
            highlight_cache: None,
            live_cache: None,
            last_edit: None,
            auto_named: false,
            disk: None,
            disk_hash: None,
            conflict: false,
        }
    }

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let snap = DiskSnapshot::read(path)?;
        let mut doc = Self::new_untitled(0);
        doc.path = Some(path.to_path_buf());
        doc.apply_disk(snap);
        Ok(doc)
    }

    /// 디스크에서 읽은 내용으로 바꿉니다 (앱에서 고친 내용은 버려짐).
    fn apply_disk(&mut self, snap: DiskSnapshot) {
        let (text, encoding, line_ending) = parse(&snap.bytes);
        // 줄바꿈이 없는 파일이면: 처음 열 때는 플랫폼 기본값, 다시 읽을 때는 쓰던 방식 그대로
        let first_load = self.disk_hash.is_none() && self.text.is_empty();
        self.line_ending =
            line_ending.unwrap_or(if first_load { LineEnding::platform_default() } else { self.line_ending });
        self.text = text;
        self.encoding = encoding;
        self.dirty = false;
        self.conflict = false;
        self.last_edit = None;
        self.highlight_cache = None;
        self.live_cache = None;
        self.remember_disk(&snap);
    }

    fn remember_disk(&mut self, snap: &DiskSnapshot) {
        self.disk = snap.stamp;
        self.disk_hash = Some(content_hash(&snap.bytes));
    }

    /// 마지막으로 읽거나 저장한 뒤 파일 내용이 밖에서 바뀌었으면 새 내용을 돌려줍니다.
    /// 수정 시각만 바뀌고 내용이 같으면 (동기화 프로그램 등) 기억만 새로 하고 `None`.
    fn changed_on_disk(&mut self) -> Option<DiskSnapshot> {
        let path = self.path.as_ref()?;
        // 파일이 없어졌으면 (지워졌거나 이름이 바뀜) 건드리지 않습니다. 다음 저장 때 다시 생깁니다.
        let stamp = DiskStamp::of(path)?;
        if self.disk == Some(stamp) {
            return None;
        }
        let snap = DiskSnapshot::read(path).ok()?;
        if self.disk_hash == Some(content_hash(&snap.bytes)) || parse(&snap.bytes).0 == self.text {
            self.remember_disk(&snap);
            return None;
        }
        Some(snap)
    }

    /// 파일이 밖에서 바뀌었는지 확인합니다. 앱에서 고친 내용이 없으면 조용히 다시 읽고,
    /// 있으면 충돌로 표시해 자동 저장을 멈춥니다. 이미 충돌 상태면 아무것도 하지 않습니다.
    pub fn check_disk(&mut self) -> DiskEvent {
        if self.conflict {
            return DiskEvent::Unchanged;
        }
        let Some(snap) = self.changed_on_disk() else { return DiskEvent::Unchanged };
        if self.dirty {
            self.conflict = true;
            DiskEvent::Conflict
        } else {
            let old_text = std::mem::take(&mut self.text);
            self.apply_disk(snap);
            DiskEvent::Reloaded { old_text }
        }
    }

    /// 충돌 해결: 디스크의 내용으로 다시 불러옵니다.
    pub fn reload(&mut self) -> std::io::Result<()> {
        let path = self.path.clone().ok_or(std::io::ErrorKind::NotFound)?;
        let snap = DiskSnapshot::read(&path)?;
        self.apply_disk(snap);
        Ok(())
    }

    /// 충돌 해결: 앱의 내용을 유지합니다. 지금 디스크의 내용을 "확인함"으로 기억해서
    /// 다음 자동 저장이 앱의 내용으로 덮어씁니다.
    pub fn keep_mine(&mut self) {
        if let Some(path) = &self.path
            && let Ok(snap) = DiskSnapshot::read(path)
        {
            self.remember_disk(&snap);
        }
        self.conflict = false;
        self.mark_changed();
    }

    /// 세션에서 복원한 (저장 안 된) 문서: 지난번에 기억한 파일 상태를 다시 씁니다.
    /// 모르면 지금 디스크 상태를 기준으로 삼습니다.
    pub fn restore_disk(&mut self, stamp: Option<DiskStamp>) {
        let Some(path) = &self.path else { return };
        self.disk = stamp.or_else(|| DiskStamp::of(path));
        self.disk_hash = None;
    }

    /// 현재 인코딩으로 저장합니다. 해당 인코딩으로 표현할 수 없는 문자가 있으면
    /// UTF-8로 바꿔 저장하고 `Ok(true)`를 돌려줍니다.
    pub fn save_to(&mut self, path: &Path) -> std::io::Result<bool> {
        let text = match self.line_ending {
            LineEnding::Lf => std::borrow::Cow::Borrowed(self.text.as_str()),
            LineEnding::CrLf => std::borrow::Cow::Owned(self.text.replace('\n', "\r\n")),
        };
        let (bytes, fell_back) = match encode(&text, self.encoding) {
            Some(bytes) => (bytes, false),
            None => (encode(&text, TextEncoding::Utf8).unwrap_or_default(), true),
        };
        if fell_back {
            self.encoding = TextEncoding::Utf8;
        }

        // 임시 파일에 쓴 뒤 교체해서, 저장 도중 문제가 생겨도 원본이 망가지지 않게 합니다.
        let tmp = path.with_extension(format!("{}.tbtmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
        std::fs::write(&tmp, &bytes)?;
        if let Err(err) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            // 다른 프로세스가 파일을 잡고 있는 경우 등: 직접 덮어쓰기로 재시도
            std::fs::write(path, &bytes).map_err(|_| err)?;
        }

        self.path = Some(path.to_path_buf());
        self.dirty = false;
        self.conflict = false;
        self.remember_disk(&DiskSnapshot { stamp: DiskStamp::of(path), bytes });
        Ok(fell_back)
    }

    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => {
                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
            }
            None if self.untitled_index <= 1 => "제목 없음".to_owned(),
            None => format!("제목 없음 {}", self.untitled_index),
        }
    }

    pub fn is_markdown(&self) -> bool {
        match self.path.as_ref().and_then(|p| p.extension()).and_then(|e| e.to_str()) {
            Some(ext) => matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown" | "mkd" | "mdx"),
            // 새 문서는 마크다운으로 취급 (일반 텍스트도 그대로 잘 보입니다)
            None => true,
        }
    }

    pub fn dir(&self) -> Option<PathBuf> {
        self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf)
    }

    /// 텍스트가 바깥에서 바뀌었을 때(찾아 바꾸기, 서식 도구 등) 호출합니다.
    pub fn mark_changed(&mut self) {
        self.dirty = true;
        self.last_edit = Some(std::time::Instant::now());
    }

    /// 아무 내용도 없고 저장되지 않은 새 문서인지 (파일을 열 때 이 탭을 재사용)
    pub fn is_pristine(&self) -> bool {
        self.path.is_none() && self.text.is_empty() && !self.dirty
    }
}

/// 파일 내용 → (`\n`으로 정규화한 텍스트, 인코딩, 줄바꿈 방식 — 줄바꿈이 없으면 `None`)
fn parse(bytes: &[u8]) -> (String, TextEncoding, Option<LineEnding>) {
    let (text, encoding) = decode(bytes);
    let line_ending = if text.contains("\r\n") {
        Some(LineEnding::CrLf)
    } else if text.contains('\n') {
        Some(LineEnding::Lf)
    } else {
        None
    };
    (text.replace("\r\n", "\n"), encoding, line_ending)
}

fn content_hash(bytes: &[u8]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// 텍스트가 `old`에서 `new`로 바뀌었을 때 커서 위치(글자 단위)를 옮깁니다.
/// 바뀐 곳보다 앞이면 그대로, 뒤면 길이 차이만큼 밀고, 바뀐 곳 안이면 바뀐 부분의 끝으로.
pub fn remap_char_index(old: &str, new: &str, index: usize) -> usize {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let prefix = old_chars.iter().zip(&new_chars).take_while(|(a, b)| a == b).count();
    let max_suffix = old_chars.len().min(new_chars.len()) - prefix;
    let suffix =
        old_chars.iter().rev().zip(new_chars.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    let index = index.min(old_chars.len());
    if index <= prefix {
        index
    } else if index >= old_chars.len() - suffix {
        index - (old_chars.len() - suffix) + (new_chars.len() - suffix)
    } else {
        new_chars.len() - suffix
    }
}

fn decode(bytes: &[u8]) -> (String, TextEncoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), TextEncoding::Utf8Bom);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (cow, _) = encoding_rs::UTF_16LE.decode_without_bom_handling(rest);
        return (cow.into_owned(), TextEncoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (cow, _) = encoding_rs::UTF_16BE.decode_without_bom_handling(rest);
        return (cow.into_owned(), TextEncoding::Utf16Be);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), TextEncoding::Utf8),
        Err(_) => {
            // UTF-8이 아니면 한국어 Windows 기본 인코딩(CP949)으로 해석
            let (cow, _) = encoding_rs::EUC_KR.decode_without_bom_handling(bytes);
            (cow.into_owned(), TextEncoding::EucKr)
        }
    }
}

/// 인코딩할 수 없는 문자가 있으면 `None`
fn encode(text: &str, encoding: TextEncoding) -> Option<Vec<u8>> {
    match encoding {
        TextEncoding::Utf8 => Some(text.as_bytes().to_vec()),
        TextEncoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(text.as_bytes());
            Some(v)
        }
        TextEncoding::Utf16Le => {
            let mut v = vec![0xFF, 0xFE];
            for u in text.encode_utf16() {
                v.extend_from_slice(&u.to_le_bytes());
            }
            Some(v)
        }
        TextEncoding::Utf16Be => {
            let mut v = vec![0xFE, 0xFF];
            for u in text.encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            Some(v)
        }
        TextEncoding::EucKr => {
            let (bytes, _, had_errors) = encoding_rs::EUC_KR.encode(text);
            (!had_errors).then(|| bytes.into_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encodings() {
        let text = "안녕하세요 Hello\n둘째 줄";
        for enc in TextEncoding::ALL {
            let bytes = encode(text, enc).unwrap();
            let (decoded, detected) = decode(&bytes);
            assert_eq!(decoded, text, "{enc:?}");
            assert_eq!(detected, enc);
        }
    }

    #[test]
    fn euc_kr_rejects_unmappable() {
        assert!(encode("이모지 😀", TextEncoding::EucKr).is_none());
    }

    /// 테스트마다 따로 쓰는 임시 파일
    fn temp_file(name: &str, content: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tb-notepad-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    /// 밖에서 파일을 고친 것처럼 쓰고, 수정 시각도 확실히 다르게 합니다.
    fn write_externally(path: &Path, content: &str, secs_later: u64) {
        std::fs::write(path, content).unwrap();
        let t = SystemTime::now() + std::time::Duration::from_secs(secs_later);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn clean_doc_reloads_silently() {
        let path = temp_file("clean.md", "첫 줄\n둘째 줄\n");
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.check_disk(), DiskEvent::Unchanged);

        write_externally(&path, "첫 줄\nAI가 쓴 줄\n둘째 줄\n", 5);
        assert_eq!(doc.check_disk(), DiskEvent::Reloaded { old_text: "첫 줄\n둘째 줄\n".into() });
        assert_eq!(doc.text, "첫 줄\nAI가 쓴 줄\n둘째 줄\n");
        assert!(!doc.dirty && !doc.conflict);
        assert_eq!(doc.check_disk(), DiskEvent::Unchanged);
    }

    #[test]
    fn same_size_change_is_detected() {
        let path = temp_file("same-size.md", "aaaa");
        let mut doc = Document::open(&path).unwrap();
        write_externally(&path, "bbbb", 5);
        assert!(matches!(doc.check_disk(), DiskEvent::Reloaded { .. }));
        assert_eq!(doc.text, "bbbb");
    }

    #[test]
    fn touch_without_content_change_is_ignored() {
        let path = temp_file("touch.md", "그대로\n");
        let mut doc = Document::open(&path).unwrap();
        doc.text.push_str("앱에서 씀");
        doc.mark_changed();
        // 동기화 프로그램처럼 같은 내용을 다시 써서 시각만 바뀜
        write_externally(&path, "그대로\n", 5);
        assert_eq!(doc.check_disk(), DiskEvent::Unchanged);
        assert!(!doc.conflict);
        assert_eq!(doc.text, "그대로\n앱에서 씀");
    }

    #[test]
    fn dirty_doc_conflicts_and_blocks_until_resolved() {
        let path = temp_file("conflict.md", "원래\n");
        let mut doc = Document::open(&path).unwrap();
        doc.text = "원래\n내가 씀\n".into();
        doc.mark_changed();
        write_externally(&path, "원래\nAI가 씀\n", 5);

        assert_eq!(doc.check_disk(), DiskEvent::Conflict);
        assert!(doc.conflict && doc.dirty);
        assert_eq!(doc.text, "원래\n내가 씀\n", "충돌 때는 앱의 내용을 건드리지 않음");
        // 이미 충돌 상태면 다시 알리지 않음
        assert_eq!(doc.check_disk(), DiskEvent::Unchanged);

        doc.reload().unwrap();
        assert_eq!(doc.text, "원래\nAI가 씀\n");
        assert!(!doc.conflict && !doc.dirty);
    }

    #[test]
    fn keep_mine_lets_autosave_overwrite() {
        let path = temp_file("keep.md", "원래\n");
        let mut doc = Document::open(&path).unwrap();
        doc.text = "내 것\n".into();
        doc.mark_changed();
        write_externally(&path, "남의 것\n", 5);
        assert_eq!(doc.check_disk(), DiskEvent::Conflict);

        doc.keep_mine();
        assert!(!doc.conflict && doc.dirty);
        // 확인한 디스크 내용은 더 이상 변경으로 보지 않음
        assert_eq!(doc.check_disk(), DiskEvent::Unchanged);
        doc.save_to(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n"), "내 것\n");
    }

    #[test]
    fn own_save_is_not_an_external_change() {
        let path = temp_file("own-save.md", "a\n");
        let mut doc = Document::open(&path).unwrap();
        doc.text = "a\nb\n".into();
        doc.mark_changed();
        doc.save_to(&path).unwrap();
        assert_eq!(doc.check_disk(), DiskEvent::Unchanged);
    }

    #[test]
    fn deleted_file_is_left_alone() {
        let path = temp_file("deleted.md", "x");
        let mut doc = Document::open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(doc.check_disk(), DiskEvent::Unchanged);
        assert_eq!(doc.text, "x");
    }

    #[test]
    fn reload_keeps_crlf_and_encoding() {
        let path = temp_file("crlf.md", "a\r\nb\r\n");
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(doc.line_ending, LineEnding::CrLf);
        write_externally(&path, "a\r\nb\r\nc\r\n", 5);
        assert!(matches!(doc.check_disk(), DiskEvent::Reloaded { .. }));
        assert_eq!(doc.text, "a\nb\nc\n");
        assert_eq!(doc.line_ending, LineEnding::CrLf);
    }

    #[test]
    fn cursor_follows_text() {
        let old = "하나\n둘\n셋";
        // 커서 앞에서 줄이 추가되면 그만큼 밀림
        let new = "영\n하나\n둘\n셋";
        assert_eq!(remap_char_index(old, new, 5), 7); // "셋" 앞
        // 커서 뒤에서 바뀌면 그대로
        let new = "하나\n둘\n셋\n넷";
        assert_eq!(remap_char_index(old, new, 3), 3);
        // 바뀐 곳 바로 뒤의 커서는 바뀐 만큼 밀림
        let new = "하나\n두울\n셋";
        assert_eq!(remap_char_index(old, new, 4), 5);
        // 커서가 있던 부분이 바뀌면 바뀐 부분의 끝으로
        assert_eq!(remap_char_index("abcdef", "abXYZef", 3), 5);
        // 끝에 있던 커서는 새 끝으로
        let new = "하나";
        assert_eq!(remap_char_index(old, new, old.chars().count()), 2);
        // 범위를 넘는 위치도 안전하게
        assert_eq!(remap_char_index("ab", "abc", 99), 2);
    }
}
