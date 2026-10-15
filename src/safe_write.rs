//! 파일을 망가뜨리지 않고 바꿔 쓰기.

use std::path::Path;

/// 다른 프로그램이 파일을 잡고 있어 통째로 바꿔치기할 수 없을 때
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhenBlocked {
    /// 원본을 그대로 두고 오류 (자동 저장 · 링크 고치기 · MCP 처럼 사용자가 지켜보지 않는 저장. 나중에 다시 시도)
    Fail,
    /// 원본을 백업해 두고 제자리에 씀 (사용자가 직접 저장할 때)
    WriteInPlace,
}

/// 파일을 안전하게 바꿔 씁니다.
/// 1. 옆의 임시 파일에 다 쓴 뒤 원본과 바꿉니다 (도중에 꺼져도 원본은 그대로).
/// 2. 바꾸기가 실패하면 (다른 프로그램이 잠깐 잡고 있는 등) 조금 기다렸다 다시 해 봅니다.
/// 3. 그래도 안 되면 [`WhenBlocked::WriteInPlace`] 일 때만 원본을 백업해 두고 그 자리에 덮어쓴 뒤 읽어서 확인하고,
///    실패하면 백업으로 되돌립니다. 백업을 만들 수 없으면 원본을 건드리지 않고 오류를 돌려줍니다.
///    제자리 쓰기는 도중에 전원이 꺼지면 원본이 반쯤 쓰일 수 있으므로 (그때는 `.tbbak` 이 남음)
///    사용자가 직접 저장할 때만 씁니다. 그 밖에는 원본을 그대로 두고 오류를 돌려줍니다.
pub fn write(path: &Path, bytes: &[u8], when_blocked: WhenBlocked) -> std::io::Result<()> {
    let allow_in_place = when_blocked == WhenBlocked::WriteInPlace;
    use std::io::Write;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let tmp = path.with_extension(format!("{ext}.tbtmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    let mut last_err = None;
    // 자동 저장은 몇 초 뒤 다시 시도하므로 화면이 멈추지 않게 짧게만 기다림
    let attempts = if allow_in_place { 5 } else { 2 };
    for attempt in 0..attempts {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(30 * (attempt + 1)));
    }
    let _ = std::fs::remove_file(&tmp);
    let rename_err = last_err.unwrap_or_else(|| std::io::Error::other("rename failed"));
    if !allow_in_place || !path.exists() {
        return Err(rename_err);
    }
    // 바꿀 수 없으면 제자리 덮어쓰기. 그 전에 원본을 백업 (백업이 안 되면 원본을 건드리지 않음)
    let bak = path.with_extension(format!("{ext}.tbbak"));
    std::fs::copy(path, &bak).map_err(|_| rename_err)?;
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if std::fs::read(path)? != bytes {
            return Err(std::io::Error::other("written content does not match"));
        }
        Ok(())
    })();
    match written {
        Ok(()) => {
            let _ = std::fs::remove_file(&bak);
            Ok(())
        }
        Err(e) => {
            // 원본 내용을 되돌리고, 그것도 안 되면 백업 파일을 남겨 둠 (path.확장자.tbbak)
            if std::fs::copy(&bak, path).is_ok() {
                let _ = std::fs::remove_file(&bak);
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 다른 프로그램이 파일을 지우기(바꾸기) 못 하게 잡고 있을 때:
    /// 자동 저장은 원본을 건드리지 않고 실패, 직접 저장은 백업해 두고 제자리에 씁니다.
    #[cfg(windows)]
    #[test]
    fn safe_write_when_replace_is_blocked() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("tbn_safe_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("메모.md");
        std::fs::write(&path, "원본").unwrap();
        let files = || std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>();
        // 읽기·쓰기는 허락하지만 지우기(이름 바꾸기로 덮기)는 막는 핸들
        let share_rw = 0x1 | 0x2;
        let holder = std::fs::OpenOptions::new().read(true).share_mode(share_rw).open(&path).unwrap();
        assert!(write(&path, "자동 저장".as_bytes(), WhenBlocked::Fail).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "원본", "자동 저장은 원본을 건드리지 않음");
        assert_eq!(files().len(), 1, "임시 파일이 남지 않음: {:?}", files());
        write(&path, "새 내용".as_bytes(), WhenBlocked::WriteInPlace).unwrap();
        drop(holder);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "새 내용");
        assert_eq!(files().len(), 1, "임시 · 백업 파일이 남지 않음: {:?}", files());

        // 쓰기까지 막혀 있으면 원본은 그대로 두고 오류
        let locked = std::fs::OpenOptions::new().read(true).share_mode(0x1).open(&path).unwrap();
        assert!(write(&path, "또 다른 내용".as_bytes(), WhenBlocked::WriteInPlace).is_err());
        drop(locked);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "새 내용", "원본이 망가지지 않음");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
