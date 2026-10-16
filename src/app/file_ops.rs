//! 사이드바의 파일 작업: 이름 바꾸기, 옮기기 (링크 · 버전 기록이 따라감), 지우기.

use super::*;

impl NotepadApp {
    pub(super) fn handle_sidebar(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Open(p) => self.open_path(&p),
                Action::NewNote(dir) => self.new_note_in(&dir),
                Action::NewQuickNote => self.new_note(),
                Action::NewFolder(parent) => {
                    self.dialog = Some(Dialog::NewFolder { parent, name: t!("새 폴더").into() })
                }
                Action::AddRoot => {
                    if let Some(dir) = rfd::FileDialog::new().set_title(t!("메모 폴더 고르기")).pick_folder()
                        && !self.settings.roots.contains(&dir)
                    {
                        self.settings.roots.push(dir);
                        self.sidebar.invalidate();
                    }
                }
                Action::RemoveRoot(dir) => self.settings.roots.retain(|r| *r != dir),
                Action::Rename { from, to_name } => self.rename_path(ctx, &from, &to_name),
                Action::Delete(p) => self.dialog = Some(Dialog::ConfirmDelete(p)),
                Action::Reveal(p) => {
                    let _ = reveal_in_file_manager(&p);
                }
                Action::CopyLink(p) => self.copy_path_link(ctx, &p),
                Action::History(p) => {
                    self.open_path(&p);
                    self.open_history(&p);
                }
                Action::Move { from, to_dir } => {
                    if let Some(name) = from.file_name()
                        && from.parent() != Some(to_dir.as_path())
                    {
                        self.move_path(ctx, &from, to_dir.join(name));
                    }
                }
                Action::ActivateDoc(id) => {
                    if let Some(i) = self.docs.iter().position(|d| d.id == id) {
                        self.set_active(i);
                    }
                }
                Action::CloseDoc(id) => self.request_close_doc(id),
            }
        }
    }

    pub(super) fn rename_path(&mut self, ctx: &egui::Context, from: &Path, to_name: &str) {
        let clean = sanitize_file_name(to_name);
        if clean.is_empty() {
            return;
        }
        let Some(dir) = from.parent() else { return };
        let target = if from.is_dir() || Path::new(&clean).extension().is_some() {
            dir.join(&clean)
        } else {
            let ext = from.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "md".into());
            dir.join(format!("{clean}.{ext}"))
        };
        self.move_path(ctx, from, target);
    }

    /// 파일·폴더를 옮기거나 이름을 바꿉니다. 열린 문서의 경로, 버전 기록, 메모 속 링크(그림 포함)가 함께 따라갑니다.
    pub(super) fn move_path(&mut self, ctx: &egui::Context, from: &Path, target: PathBuf) -> bool {
        if target == from {
            return false;
        }
        if target.exists() {
            self.set_status(ctx, t!("같은 이름이 이미 있습니다"));
            return false;
        }
        if target.starts_with(from) {
            self.set_status(ctx, t!("폴더를 자기 안으로 옮길 수 없습니다"));
            return false;
        }
        // 열려 있는 문서는 먼저 저장해 두고 경로를 함께 바꿉니다.
        let ids: Vec<u64> =
            self.docs.iter().filter(|d| d.path.as_deref().is_some_and(|p| p.starts_with(from))).map(|d| d.id).collect();
        for id in &ids {
            self.save_if_needed(*id);
        }
        if let Err(err) = move_any(from, &target) {
            self.dialog = Some(Dialog::Message(tf!("옮기지 못했습니다.\n{}", err)));
            return false;
        }
        // 고정 · 숨김 · 순서도 따라감
        let roots = self.settings.roots.clone();
        self.sidebar.path_moved(&roots, from, &target);
        for d in &mut self.docs {
            if let Some(p) = &d.path
                && let Ok(rest) = p.strip_prefix(from)
            {
                d.path = Some(if rest.as_os_str().is_empty() { target.clone() } else { target.join(rest) });
                d.auto_named = false;
            }
        }
        // 버전 기록도 따라 옮김 (폴더면 안의 파일마다)
        if target.is_dir() {
            for_each_file(&target, &mut |file| {
                if let Ok(rest) = file.strip_prefix(&target) {
                    crate::history::moved(&from.join(rest), file);
                }
            });
        } else {
            crate::history::moved(from, &target);
        }
        let (fixed, failed) = self.update_links_after_move(from, &target);
        self.sidebar.invalidate();
        let roots = self.settings.roots.clone();
        self.sidebar.reveal(&target, &roots);
        if !failed.is_empty() {
            let list: Vec<String> = failed.iter().map(|p| format!("· {}", p.display())).collect();
            self.dialog = Some(Dialog::Message(tf!(
                "옮겼지만 이 메모들의 링크를 고쳐 저장하지 못했습니다:\n\n{}",
                list.join("\n")
            )));
        } else if fixed > 0 {
            self.set_status(ctx, tf!("옮겼습니다 · 링크를 고친 메모 {}개", fixed));
        }
        true
    }

    /// 옮긴 뒤 메모 폴더의 모든 메모에서 링크를 고칩니다. (고친 메모 수, 고쳤지만 저장하지 못한 메모)
    fn update_links_after_move(&mut self, from: &Path, to: &Path) -> (usize, Vec<PathBuf>) {
        let mut files = Vec::new();
        for root in &self.settings.roots {
            sidebar::walk_tree(root, &self.settings.image_folder, 0, &mut |p, is_dir| {
                if !is_dir && is_markdown_path(p) {
                    files.push(p.to_path_buf());
                }
            });
        }
        let mut fixed = 0;
        let mut failed = Vec::new();
        for file in files {
            let old_file = match file.strip_prefix(to) {
                Ok(rest) if rest.as_os_str().is_empty() => from.to_path_buf(),
                Ok(rest) => from.join(rest),
                Err(_) => file.clone(),
            };
            let (Some(old_dir), Some(new_dir)) = (old_file.parent(), file.parent()) else { continue };
            if let Some(doc) = self.docs.iter_mut().find(|d| d.path.as_deref() == Some(file.as_path())) {
                if let Some(text) = links::rebase_links(&doc.text, old_dir, new_dir, from, to) {
                    doc.text = text;
                    doc.mark_changed();
                    fixed += 1;
                }
                continue;
            }
            let Ok(mut doc) = Document::open(&file) else { continue };
            if let Some(text) = links::rebase_links(&doc.text, old_dir, new_dir, from, to) {
                doc.text = text;
                if doc.save_to(&file, WhenBlocked::Fail).is_ok() {
                    fixed += 1;
                } else {
                    failed.push(file);
                }
            }
        }
        (fixed, failed)
    }

    pub(super) fn delete_path(&mut self, ctx: &egui::Context, path: &Path) {
        match sidebar::move_to_trash(path) {
            Ok(()) => {
                let roots = self.settings.roots.clone();
                self.sidebar.path_removed(&roots, path);
                let ids: Vec<u64> = self
                    .docs
                    .iter()
                    .filter(|d| d.path.as_deref().is_some_and(|p| p.starts_with(path)))
                    .map(|d| d.id)
                    .collect();
                for id in ids {
                    self.close_doc(id);
                }
                self.sidebar.invalidate();
                self.set_status(ctx, t!("휴지통으로 옮겼습니다"));
            }
            Err(err) => self.dialog = Some(Dialog::Message(tf!("지우지 못했습니다.\n{}", err))),
        }
    }
}

/// 파일이나 폴더를 옮깁니다. 다른 드라이브로는 이름 바꾸기로 옮길 수 없으므로 (os error 17)
/// 복사한 뒤 원본을 지웁니다. 복사가 중간에 실패하면 복사한 것을 지우고 원본은 그대로 둡니다.
pub(super) fn move_any(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices || e.raw_os_error() == Some(17) => {
            if let Err(copy_err) = copy_tree(from, to) {
                let _ = if to.is_dir() { std::fs::remove_dir_all(to) } else { std::fs::remove_file(to) };
                return Err(copy_err);
            }
            if from.is_dir() { std::fs::remove_dir_all(from) } else { std::fs::remove_file(from) }
        }
        Err(e) => Err(e),
    }
}

/// 파일 하나, 또는 폴더를 통째로 복사 (수정한 날짜는 복사본도 같게)
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to)?;
        if let Ok(modified) = std::fs::metadata(from).and_then(|m| m.modified())
            && let Ok(f) = std::fs::File::options().write(true).open(to)
        {
            let _ = f.set_modified(modified);
        }
        Ok(())
    }
}

/// 폴더 안의 모든 파일 (하위 폴더 포함)
fn for_each_file(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => for_each_file(&path, f),
            Ok(t) if t.is_file() => f(&path),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_whole_folders() {
        let base = std::env::temp_dir().join(format!("tbn_copytree_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        std::fs::write(base.join("a/메모.md"), "하나").unwrap();
        std::fs::write(base.join("a/b/둘.md"), "둘").unwrap();
        copy_tree(&base.join("a"), &base.join("c")).unwrap();
        assert_eq!(std::fs::read_to_string(base.join("c/메모.md")).unwrap(), "하나");
        assert_eq!(std::fs::read_to_string(base.join("c/b/둘.md")).unwrap(), "둘");
        // 같은 드라이브면 그냥 옮김
        move_any(&base.join("c"), &base.join("d")).unwrap();
        assert!(!base.join("c").exists() && base.join("d/b/둘.md").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
