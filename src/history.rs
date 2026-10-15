//! 버전 기록: 저장하기 직전의 파일을 가끔씩 복사해 두었다가 "이전 버전"으로 되돌릴 수 있게 합니다.
//!
//! - 위치: 설정 폴더의 `history/<경로 해시>/` (메모 폴더를 어지럽히지 않고, 동기화 폴더에도 올라가지 않음)
//! - 언제: 마지막 기록에서 5분이 지났거나, 내용이 한꺼번에 많이 줄었을 때 (실수로 지운 경우)
//! - 메모마다 최근 50개까지 (원래 파일 그대로 복사하므로 인코딩·줄 끝도 그대로)
//!
//! 앱과 `--mcp`(AI) 저장이 모두 [`crate::document::Document::save_to`] 를 거치므로 둘 다 기록됩니다.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 이만큼 지나야 다음 기록을 남김
const MIN_GAP: Duration = Duration::from_secs(5 * 60);
/// 메모마다 남기는 개수
const KEEP: usize = 50;
/// 이 크기를 넘는 파일은 기록하지 않음
const MAX_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub file: PathBuf,
    pub time: SystemTime,
    pub size: u64,
}

pub fn history_root() -> PathBuf {
    crate::data_dir().join("history")
}

/// 경로마다 고정된 폴더 이름 (대소문자 무시, FNV-1a)
fn key(path: &Path) -> String {
    let s = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().to_lowercase();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.replace('/', "\\").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn dir_for(root: &Path, path: &Path) -> PathBuf {
    root.join(key(path))
}

/// 이 메모의 기록 (새것부터)
pub fn list_in(root: &Path, path: &Path) -> Vec<Snapshot> {
    let Ok(rd) = std::fs::read_dir(dir_for(root, path)) else { return Vec::new() };
    let mut out: Vec<Snapshot> = rd
        .flatten()
        .filter(|e| e.file_name() != "path.txt")
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some(Snapshot { file: e.path(), time: m.modified().ok()?, size: m.len() })
        })
        .collect();
    out.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| b.file.cmp(&a.file)));
    out
}

pub fn list(path: &Path) -> Vec<Snapshot> {
    list_in(&history_root(), path)
}

/// 저장하기 직전에 부릅니다: 지금 디스크에 있는 파일을 (필요하면) 기록으로 복사합니다.
pub fn record_before_save(path: &Path, new_bytes: &[u8]) {
    // 테스트가 실제 설정 폴더에 기록을 남기지 않도록
    if cfg!(test) {
        return;
    }
    let _ = record_in(&history_root(), path, new_bytes, SystemTime::now());
}

/// 되돌리기 직전처럼 꼭 남겨야 할 때: 시간 간격과 상관없이 지금 파일을 기록합니다.
pub fn record_now(path: &Path) {
    if cfg!(test) {
        return;
    }
    let _ = record(&history_root(), path, &[], SystemTime::now(), true);
}

pub fn record_in(root: &Path, path: &Path, new_bytes: &[u8], now: SystemTime) -> std::io::Result<bool> {
    record(root, path, new_bytes, now, false)
}

fn record(root: &Path, path: &Path, new_bytes: &[u8], now: SystemTime, force: bool) -> std::io::Result<bool> {
    let Ok(meta) = std::fs::metadata(path) else { return Ok(false) };
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Ok(false);
    }
    let old = std::fs::read(path)?;
    if old == new_bytes || old.is_empty() {
        return Ok(false);
    }
    let existing = list_in(root, path);
    if let Some(latest) = existing.first() {
        let recent = now.duration_since(latest.time).map_or(true, |d| d < MIN_GAP);
        // 한꺼번에 많이 지웠으면 (실수일 수 있으니) 시간과 상관없이 남김
        let big_cut = old.len() > 300 && new_bytes.len() * 10 < old.len() * 6;
        if recent && !big_cut && !force {
            return Ok(false);
        }
        if std::fs::read(&latest.file).is_ok_and(|b| b == old) {
            return Ok(false);
        }
    }
    let dir = dir_for(root, path);
    std::fs::create_dir_all(&dir)?;
    let _ = std::fs::write(dir.join("path.txt"), path.to_string_lossy().as_bytes());
    let stamp = chrono::DateTime::<chrono::Local>::from(now).format("%Y%m%d-%H%M%S").to_string();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("txt");
    let mut file = dir.join(format!("{stamp}.{ext}"));
    let mut n = 2;
    while file.exists() {
        file = dir.join(format!("{stamp}-{n}.{ext}"));
        n += 1;
    }
    std::fs::write(&file, &old)?;
    // 남긴 파일의 시각을 `now` 로 (목록 순서 = 기록한 순서)
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&file) {
        let _ = f.set_modified(now);
    }
    for extra in list_in(root, path).into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(extra.file);
    }
    Ok(true)
}

/// 이름을 바꾸거나 옮긴 메모의 기록도 따라가게 합니다.
pub fn moved(from: &Path, to: &Path) {
    if cfg!(test) {
        return;
    }
    let root = history_root();
    let (a, b) = (dir_for(&root, from), dir_for(&root, to));
    if a.is_dir() && !b.exists() && std::fs::rename(&a, &b).is_ok() {
        let _ = std::fs::write(b.join("path.txt"), to.to_string_lossy().as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_with_gap_and_big_cuts() {
        let base = std::env::temp_dir().join(format!("tbn_hist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("history");
        std::fs::create_dir_all(&base).unwrap();
        let note = base.join("메모.md");
        let t0 = SystemTime::now();
        let min = |m: u64| t0 + Duration::from_secs(m * 60);

        assert!(!record_in(&root, &note, b"x", t0).unwrap(), "아직 파일이 없음");
        std::fs::write(&note, "첫 내용").unwrap();
        assert!(record_in(&root, &note, "둘째".as_bytes(), t0).unwrap(), "첫 기록");
        std::fs::write(&note, "둘째").unwrap();
        assert!(!record_in(&root, &note, "셋째".as_bytes(), min(1)).unwrap(), "5분 안에는 다시 안 남김");
        std::fs::write(&note, "셋째").unwrap();
        assert!(record_in(&root, &note, "넷째".as_bytes(), min(6)).unwrap(), "5분 뒤에는 남김");

        // 많이 지우면 바로 남김
        let long = "가".repeat(400);
        std::fs::write(&note, &long).unwrap();
        assert!(record_in(&root, &note, "짧게".as_bytes(), min(7)).unwrap());

        let list = list_in(&root, &note);
        assert_eq!(list.len(), 3);
        assert_eq!(std::fs::read_to_string(&list[0].file).unwrap(), long, "새것부터");
        assert_eq!(std::fs::read_to_string(&list[2].file).unwrap(), "첫 내용");
        let _ = std::fs::remove_dir_all(&base);
    }
}
