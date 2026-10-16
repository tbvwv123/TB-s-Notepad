//! 메모 폴더의 사이드바 정보: 상단 고정, 숨김, 끌어서 정한 순서.
//!
//! 메모 폴더(사이드바의 최상위 폴더)마다 `.tbnotepad.json` 하나에 저장합니다. 점으로 시작해 사이드바에는
//! 보이지 않고, 메모 폴더와 함께 동기화되므로 다른 PC 에서도 같은 고정 · 순서가 유지됩니다.
//! 경로는 메모 폴더 기준 상대 경로를 `/` 로 이어 씁니다 (메모 폴더 자체는 빈 글자).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE: &str = ".tbnotepad.json";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderMeta {
    /// 상단에 고정한 메모 · 폴더 (고정한 순서대로 맨 위에)
    pub pinned: Vec<String>,
    /// 숨긴 메모 · 폴더
    pub hidden: Vec<String>,
    /// 폴더마다 끌어서 정한 순서 (이름들). 정렬을 바꾸면 지워집니다.
    pub order: BTreeMap<String, Vec<String>>,
}

/// `root` 기준 상대 경로 (`a/b.md`). `root` 밖이면 `None`
pub fn rel_key(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
}

fn under(key: &str, prefix: &str) -> bool {
    key == prefix || (key.len() > prefix.len() && key.starts_with(prefix) && key.as_bytes()[prefix.len()] == b'/')
}

/// `key` 가 `from` 이나 그 안이면 앞부분을 `to` 로
fn rebase(key: &str, from: &str, to: &str) -> Option<String> {
    under(key, from).then(|| format!("{to}{}", &key[from.len()..]))
}

fn parent_and_name(key: &str) -> (&str, &str) {
    key.rsplit_once('/').unwrap_or(("", key))
}

impl FolderMeta {
    pub fn load(root: &Path) -> FolderMeta {
        std::fs::read_to_string(root.join(FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// 저장 (아무것도 없으면 파일을 지움)
    pub fn save(&self, root: &Path) {
        let path = root.join(FILE);
        if *self == FolderMeta::default() {
            let _ = std::fs::remove_file(path);
        } else if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = crate::safe_write::write(&path, json.as_bytes(), crate::safe_write::WhenBlocked::Fail);
        }
    }

    pub fn is_pinned(&self, key: &str) -> bool {
        self.pinned.iter().any(|k| k == key)
    }

    /// 이것이나 이것을 담은 폴더가 숨겨졌는지
    pub fn is_hidden(&self, key: &str) -> bool {
        self.hidden.iter().any(|h| under(key, h))
    }

    pub fn set_pinned(&mut self, key: &str, on: bool) {
        self.pinned.retain(|k| k != key);
        if on {
            self.pinned.push(key.to_owned());
        }
    }

    pub fn set_hidden(&mut self, key: &str, on: bool) {
        self.hidden.retain(|k| k != key);
        if on {
            self.hidden.push(key.to_owned());
        }
    }

    /// 폴더 `dir` 안의 순서를 정합니다 (이름들)
    pub fn set_order(&mut self, dir: &str, names: Vec<String>) {
        self.order.insert(dir.to_owned(), names);
    }

    /// 파일 · 폴더를 옮기거나 이름을 바꿈: 고정 · 숨김 · 순서가 따라갑니다
    pub fn moved(&mut self, from: &str, to: &str) {
        for list in [&mut self.pinned, &mut self.hidden] {
            for k in list.iter_mut() {
                if let Some(n) = rebase(k, from, to) {
                    *k = n;
                }
            }
        }
        let order = std::mem::take(&mut self.order);
        self.order = order.into_iter().map(|(k, v)| (rebase(&k, from, to).unwrap_or(k), v)).collect();
        // 같은 폴더에서 이름만 바뀌었으면 순서 안의 이름도
        let ((from_dir, from_name), (to_dir, to_name)) = (parent_and_name(from), parent_and_name(to));
        if let Some(names) = self.order.get_mut(from_dir) {
            if from_dir == to_dir {
                for n in names.iter_mut() {
                    if n == from_name {
                        *n = to_name.to_owned();
                    }
                }
            } else {
                names.retain(|n| n != from_name);
            }
        }
    }

    /// 지웠거나 다른 메모 폴더로 옮겨 나감
    pub fn removed(&mut self, key: &str) {
        self.pinned.retain(|k| !under(k, key));
        self.hidden.retain(|k| !under(k, key));
        self.order.retain(|k, _| !under(k, key));
        let (dir, name) = parent_and_name(key);
        if let Some(names) = self.order.get_mut(dir) {
            names.retain(|n| n != name);
        }
    }

    /// 다른 메모 폴더에서 옮겨 온 것의 고정 · 숨김을 이어받음 (`from` 쪽 정보에서)
    pub fn adopt(&mut self, other: &FolderMeta, from: &str, to: &str) {
        if other.is_pinned(from) {
            self.set_pinned(to, true);
        }
        if other.hidden.iter().any(|h| h == from) {
            self.set_hidden(to, true);
        }
    }
}

/// 메모 폴더들 중 `path` 가 들어 있는 것
pub fn root_of<'a>(roots: &'a [PathBuf], path: &Path) -> Option<&'a PathBuf> {
    roots.iter().filter(|r| path.starts_with(r)).max_by_key(|r| r.as_os_str().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_hiding() {
        let root = Path::new("C:/notes");
        assert_eq!(rel_key(root, Path::new("C:/notes/a/b.md")).as_deref(), Some("a/b.md"));
        assert_eq!(rel_key(root, root).as_deref(), Some(""));
        let mut m = FolderMeta::default();
        m.set_hidden("비밀", true);
        assert!(m.is_hidden("비밀") && m.is_hidden("비밀/일기.md"));
        assert!(!m.is_hidden("비밀번호.md"), "이름이 같은 앞부분일 뿐이면 아님");
        m.set_hidden("비밀", false);
        assert!(!m.is_hidden("비밀/일기.md"));
    }

    #[test]
    fn follows_moves_and_renames() {
        let mut m = FolderMeta::default();
        m.set_pinned("a/x.md", true);
        m.set_hidden("a/sub", true);
        m.set_order("a", vec!["sub".into(), "x.md".into(), "y.md".into()]);
        m.set_order("a/sub", vec!["1.md".into(), "2.md".into()]);
        // 같은 폴더 안에서 이름 바꾸기
        m.moved("a/x.md", "a/z.md");
        assert!(m.is_pinned("a/z.md") && !m.is_pinned("a/x.md"));
        assert_eq!(m.order["a"], ["sub", "z.md", "y.md"]);
        // 폴더째 옮기기
        m.moved("a/sub", "b/sub2");
        assert!(m.is_hidden("b/sub2/1.md"));
        assert_eq!(m.order["b/sub2"], ["1.md", "2.md"]);
        assert_eq!(m.order["a"], ["z.md", "y.md"], "옮겨 나간 것은 순서에서 빠짐");
        // 지우기
        m.removed("a/z.md");
        assert!(m.pinned.is_empty());
        assert_eq!(m.order["a"], ["y.md"]);
    }

    #[test]
    fn saves_only_when_needed() {
        let root = std::env::temp_dir().join(format!("tbn_meta_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut m = FolderMeta::default();
        m.save(&root);
        assert!(!root.join(FILE).exists(), "빈 정보는 파일을 만들지 않음");
        m.set_pinned("가.md", true);
        m.save(&root);
        assert_eq!(FolderMeta::load(&root), m);
        m.set_pinned("가.md", false);
        m.save(&root);
        assert!(!root.join(FILE).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
