//! 사이드바 파일 도우미: 메모 파일 판별, 폴더 훑기, 자연스러운 이름 순서, 휴지통, 기본 메모 폴더.

use super::*;

pub fn is_note(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| NOTE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

pub(super) fn display_name(path: &Path, is_dir: bool) -> String {
    let os = if is_dir { path.file_name() } else { path.file_stem() };
    os.map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

pub(super) fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// 폴더 안의 하위 폴더와 메모 파일을 모두 훑습니다 (사이드바에 보이는 것과 같은 것만, 깊이·개수 제한)
pub fn walk_tree(dir: &Path, skip_name: &str, depth: usize, f: &mut dyn FnMut(&Path, bool)) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten().take(3000) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with('~') {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if !SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) && !name.eq_ignore_ascii_case(skip_name) {
                f(&path, true);
                walk_tree(&path, skip_name, depth + 1, f);
            }
        } else if is_note(&path) {
            f(&path, false);
        }
    }
}

/// 폴더 안의 메모 파일을 모두 훑습니다 (깊이·개수 제한)
pub(super) fn walk(dir: &Path, depth: usize, f: &mut dyn FnMut(&Path)) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut count = 0;
    for entry in rd.flatten() {
        count += 1;
        if count > 3000 {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if !SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                walk(&path, depth + 1, f);
            }
        } else if is_note(&path) {
            f(&path);
        }
    }
}

/// "메모 2" < "메모 10" 처럼 숫자를 숫자로 비교
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let o = na.len().cmp(&nb.len()).then(na.cmp(&nb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// 파일·폴더를 휴지통으로 (Windows). 다른 OS에서는 바로 지웁니다.
pub fn move_to_trash(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{
            FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, SHFILEOPSTRUCTW, SHFileOperationW,
        };
        // 경로 목록은 \0\0 으로 끝나야 합니다.
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.extend([0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
            ..Default::default()
        };
        // SAFETY: op의 포인터들은 이 함수가 끝날 때까지 살아 있습니다.
        let rc = unsafe { SHFileOperationW(&mut op) };
        if rc != 0 || op.fAnyOperationsAborted != 0 {
            return Err(std::io::Error::other(tf!("휴지통으로 옮기지 못했습니다 (코드 {})", rc)));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
    }
}

/// 기본 메모 폴더: 문서\TB Notes
pub fn default_notes_dir() -> PathBuf {
    // 테스트 등으로 저장 폴더를 따로 지정했으면 그 안에 만듭니다.
    if let Some(dir) = std::env::var_os("TB_NOTEPAD_DATA") {
        return PathBuf::from(dir).join("TB Notes");
    }
    documents_dir().join("TB Notes")
}

pub(super) fn documents_dir() -> PathBuf {
    #[cfg(windows)]
    {
        // "Shell Folders" 는 OneDrive 등으로 옮긴 실제 문서 폴더 경로를 담고 있습니다.
        if let Ok(key) = winreg::HKCU.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders")
            && let Ok(p) = key.get_value::<String, _>("Personal")
            && !p.is_empty()
        {
            return PathBuf::from(p);
        }
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Documents")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v = vec!["메모 10", "메모 2", "Apple", "메모 1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Apple", "메모 1", "메모 2", "메모 10"]);
    }
}
