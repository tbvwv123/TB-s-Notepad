//! 문서 열기·닫기·저장, 자동 저장, 밖에서 바뀐 파일 다시 읽기, 사이드바에서 온 파일 작업.

use super::*;

impl NotepadApp {
    /// 다른 문서로 바꿀 때 이전 문서를 정리합니다:
    /// 아무것도 안 쓴 새 메모 파일은 지우고, 폴더 안의 저장된 문서는 메모리에서 내립니다
    /// (사이드바에서 언제든 다시 열 수 있음).
    pub(super) fn set_active(&mut self, index: usize) {
        if index == self.active || index >= self.docs.len() {
            self.active = index.min(self.docs.len() - 1);
            return;
        }
        let prev_id = self.docs[self.active].id;
        let new_id = self.docs[index].id;
        self.active = index;
        self.save_if_needed(prev_id);
        if let Some(i) = self.docs.iter().position(|d| d.id == prev_id) {
            let d = &self.docs[i];
            let empty_new_note = d.auto_named && d.text.trim().is_empty() && !d.dirty;
            if empty_new_note {
                if let Some(p) = &d.path
                    && std::fs::metadata(p).is_ok_and(|m| m.len() == 0)
                {
                    let _ = std::fs::remove_file(p);
                    self.sidebar.invalidate();
                }
                self.docs.remove(i);
            } else if !d.dirty && d.path.as_deref().is_some_and(|p| self.in_roots(p)) {
                self.docs.remove(i);
            }
        }
        self.active = self.docs.iter().position(|d| d.id == new_id).unwrap_or(0);
        self.focus_editor = true;
        self.find.current = None;
    }

    pub(super) fn push_and_activate(&mut self, doc: Document) {
        self.docs.push(doc);
        self.set_active(self.docs.len() - 1);
    }

    pub(super) fn new_untitled(&mut self) {
        self.untitled_counter += 1;
        let doc = Document::new_untitled(self.untitled_counter);
        if self.docs.is_empty() {
            self.docs.push(doc);
            self.active = 0;
        } else {
            self.push_and_activate(doc);
        }
        self.focus_editor = true;
    }

    /// "새 메모": 지금 문서가 있는 폴더(또는 첫 번째 메모 폴더)에 파일을 바로 만듭니다.
    pub(super) fn new_note(&mut self) {
        let dir = self
            .doc()
            .dir()
            .filter(|d| self.in_roots(d))
            .or_else(|| self.settings.roots.iter().find(|r| r.exists()).cloned());
        match dir {
            Some(dir) => self.new_note_in(&dir),
            None => self.new_untitled(),
        }
    }

    pub(super) fn new_note_in(&mut self, dir: &Path) {
        let path = unique_path(dir, t!("새 메모"), "md");
        match std::fs::write(&path, "") {
            Ok(()) => {
                if let Ok(mut doc) = Document::open(&path) {
                    doc.auto_named = true;
                    self.push_and_activate(doc);
                }
                self.sidebar.invalidate();
                let roots = self.settings.roots.clone();
                self.sidebar.reveal(&path, &roots);
                self.focus_editor = true;
            }
            Err(err) => {
                self.dialog = Some(Dialog::Message(tf!("메모를 만들 수 없습니다.\n{}\n\n{}", dir.display(), err)))
            }
        }
    }

    pub(super) fn open_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter(t!("텍스트 / 마크다운"), TEXT_EXTENSIONS)
            .add_filter(t!("모든 파일"), &["*"]);
        if let Some(dir) = self.doc().dir() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(paths) = dlg.pick_files() {
            for p in paths {
                self.open_path(&p);
            }
        }
    }

    pub(super) fn open_path(&mut self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_file(p, &canonical))) {
            self.set_active(i);
            return;
        }
        match Document::open(path) {
            Ok(doc) => {
                if self.docs.len() == 1 && self.docs[0].is_pristine() {
                    self.docs[0] = doc;
                    self.active = 0;
                } else {
                    self.push_and_activate(doc);
                }
                self.settings.push_recent(path.to_path_buf());
                self.focus_editor = true;
            }
            Err(err) => {
                self.settings.recent_files.retain(|p| p != path);
                self.dialog = Some(Dialog::Message(tf!("파일을 열 수 없습니다.\n{}\n\n{}", path.display(), err)));
            }
        }
    }

    pub(super) fn request_close_doc(&mut self, id: u64) {
        let Some(i) = self.docs.iter().position(|d| d.id == id) else { return };
        self.save_if_needed(id);
        if self.docs[i].dirty {
            self.set_active(i);
            self.dialog = Some(Dialog::CloseDoc(id));
        } else {
            self.close_doc(id);
        }
    }

    pub(super) fn close_doc(&mut self, id: u64) {
        let Some(i) = self.docs.iter().position(|d| d.id == id) else { return };
        let active_id = self.doc().id;
        self.docs.remove(i);
        if self.docs.is_empty() {
            self.untitled_counter = 0;
            self.new_untitled();
            return;
        }
        self.active =
            self.docs.iter().position(|d| d.id == active_id).unwrap_or(i.saturating_sub(1).min(self.docs.len() - 1));
        self.focus_editor = true;
    }

    /// 저장에 성공하면 `true`. 대화상자에서 취소하면 `false`.
    pub(super) fn save_doc(&mut self, ctx: &egui::Context, index: usize, save_as: bool) -> bool {
        let path = match (&self.docs[index].path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let doc = &self.docs[index];
                let default_name = match &doc.path {
                    Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    None => format!("{}.md", title_from_text(&doc.text).unwrap_or_else(|| doc.title())),
                };
                let mut dlg = rfd::FileDialog::new()
                    .set_file_name(&default_name)
                    .add_filter(t!("마크다운"), &["md"])
                    .add_filter(t!("텍스트"), &["txt"])
                    .add_filter(t!("모든 파일"), &["*"]);
                if let Some(dir) = doc.dir().or_else(|| self.settings.roots.first().cloned()) {
                    dlg = dlg.set_directory(dir);
                }
                match dlg.save_file() {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        // 다른 곳에 있는 그림을 문서 폴더로 복사해 함께 보관 (다른 이름으로 저장해 폴더가 바뀐 경우 포함)
        if self.settings.keep_images_with_doc
            && is_markdown_path(&path)
            && let Some(new_dir) = path.parent()
        {
            let old_base = self.docs[index].dir();
            let folder = self.settings.image_folder.clone();
            if let Some(text) = markdown::collect_images(&self.docs[index].text, old_base.as_deref(), new_dir, &folder)
            {
                self.docs[index].text = text;
            }
        }
        match self.docs[index].save_to(&path) {
            Ok(fell_back) => {
                self.docs[index].highlight_cache = None;
                self.docs[index].auto_named = false;
                self.settings.push_recent(path.clone());
                self.sidebar.invalidate();
                let name = self.docs[index].title();
                if fell_back {
                    self.set_status(
                        ctx,
                        tf!("{}: 이 인코딩으로 표현할 수 없는 글자가 있어 UTF-8로 저장했습니다", name),
                    );
                } else {
                    self.set_status(ctx, tf!("{} 저장됨", name));
                }
                true
            }
            Err(err) => {
                self.dialog = Some(Dialog::Message(tf!("저장하지 못했습니다.\n{}\n\n{}", path.display(), err)));
                false
            }
        }
    }

    pub(super) fn save_all(&mut self, ctx: &egui::Context) -> bool {
        for i in 0..self.docs.len() {
            if self.docs[i].dirty && !self.save_doc(ctx, i, false) {
                return false;
            }
        }
        true
    }

    /// 파일이 있는 문서를 조용히 저장 (자동 저장). "새 메모"는 첫 줄로 파일 이름도 맞춥니다.
    pub(super) fn save_if_needed(&mut self, id: u64) {
        if !self.settings.autosave {
            return;
        }
        let Some(i) = self.docs.iter().position(|d| d.id == id) else { return };
        let doc = &mut self.docs[i];
        let Some(path) = doc.path.clone() else { return };
        // 덮어쓰기 직전에 파일이 밖에서 바뀌지 않았는지 확인합니다. 바뀌었으면 저장하지 않고
        // 충돌로 표시해 두고, 사용자에게 물어봅니다 (show_conflict).
        if doc.dirty && !doc.conflict {
            doc.check_disk();
        }
        if doc.conflict {
            return;
        }
        if doc.auto_named
            && let Some(title) = title_from_text(&doc.text)
            && let Some(dir) = path.parent()
            && path.file_stem().is_some_and(|s| s.to_string_lossy() != title)
        {
            let target = unique_path(dir, &title, "md");
            if std::fs::rename(&path, &target).is_ok() {
                doc.path = Some(target);
                self.sidebar.invalidate();
            }
        }
        let doc = &mut self.docs[i];
        if doc.dirty
            && let Some(path) = doc.path.clone()
            && doc.save_to(&path).is_ok()
        {
            doc.last_edit = None;
        }
    }

    pub(super) fn autosave(&mut self, ctx: &egui::Context) {
        if !self.settings.autosave {
            return;
        }
        let due: Vec<u64> = self
            .docs
            .iter()
            .filter(|d| d.dirty && d.path.is_some())
            .filter(|d| d.last_edit.is_none_or(|t| t.elapsed().as_secs_f32() >= AUTOSAVE_DELAY))
            .map(|d| d.id)
            .collect();
        for id in due {
            self.save_if_needed(id);
        }
        if self.docs.iter().any(|d| d.dirty && d.path.is_some() && !d.conflict) {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    /// 창이 포커스를 얻을 때와 몇 초마다 열린 문서의 파일을 확인합니다.
    /// 앱에서 고친 내용이 없으면 조용히 다시 읽고, 있으면 충돌로 표시합니다.
    pub(super) fn check_external_changes(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let focus_gained = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true))));
        if focus_gained || now - self.last_disk_check >= DISK_CHECK_INTERVAL {
            self.last_disk_check = now;
            for i in 0..self.docs.len() {
                if let DiskEvent::Reloaded { old_text } = self.docs[i].check_disk() {
                    self.after_reload(ctx, i, &old_text);
                    let name = self.docs[i].title();
                    self.set_status(ctx, tf!("{}: 밖에서 바뀐 내용을 불러왔습니다", name));
                }
            }
        }
        // 창이 가만히 있어도 주기적으로 확인하도록
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(DISK_CHECK_INTERVAL));
    }

    /// 다시 읽은 문서의 커서/선택을 새 내용에 맞춰 옮깁니다.
    pub(super) fn after_reload(&mut self, ctx: &egui::Context, index: usize, old_text: &str) {
        let doc = &self.docs[index];
        let id = editor_id(doc.id);
        if let Some(mut state) = TextEditState::load(ctx, id)
            && let Some(range) = state.cursor.char_range()
        {
            let primary = remap_char_index(old_text, &doc.text, range.primary.index.0);
            let secondary = remap_char_index(old_text, &doc.text, range.secondary.index.0);
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(secondary), CCursor::new(primary))));
            state.store(ctx, id);
        }
        if index == self.active {
            self.find.current = None;
        }
        ctx.request_repaint();
    }

    /// 충돌한 문서가 있으면 (다른 대화상자가 없을 때) 어떻게 할지 묻습니다.
    pub(super) fn show_conflict(&mut self) {
        if self.dialog.is_none()
            && let Some(d) = self.docs.iter().find(|d| d.conflict)
        {
            self.dialog = Some(Dialog::ExternalChange(d.id));
        }
    }

    pub(super) fn export_html(&mut self, ctx: &egui::Context) {
        let doc = self.doc();
        let stem = doc
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| doc.title());
        let mut dlg = rfd::FileDialog::new().set_file_name(format!("{stem}.html")).add_filter("HTML", &["html"]);
        if let Some(dir) = doc.dir() {
            dlg = dlg.set_directory(dir);
        }
        let Some(path) = dlg.save_file() else { return };
        let html = markdown::to_html(&doc.text, &stem, &self.settings.palette());
        match std::fs::write(&path, html) {
            Ok(()) => self.set_status(ctx, tf!("{} 로 내보냈습니다", path.display())),
            Err(err) => self.dialog = Some(Dialog::Message(tf!("내보내지 못했습니다.\n{}", err))),
        }
    }

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
        if target.exists() {
            self.set_status(ctx, t!("같은 이름이 이미 있습니다"));
            return;
        }
        // 열려 있는 문서는 먼저 저장해 두고 경로를 함께 바꿉니다.
        let ids: Vec<u64> =
            self.docs.iter().filter(|d| d.path.as_deref().is_some_and(|p| p.starts_with(from))).map(|d| d.id).collect();
        for id in &ids {
            self.save_if_needed(*id);
        }
        match std::fs::rename(from, &target) {
            Ok(()) => {
                for d in &mut self.docs {
                    if let Some(p) = &d.path
                        && let Ok(rest) = p.strip_prefix(from)
                    {
                        d.path = Some(if rest.as_os_str().is_empty() { target.clone() } else { target.join(rest) });
                        d.auto_named = false;
                    }
                }
                self.sidebar.invalidate();
            }
            Err(err) => self.dialog = Some(Dialog::Message(tf!("이름을 바꾸지 못했습니다.\n{}", err))),
        }
    }

    pub(super) fn delete_path(&mut self, ctx: &egui::Context, path: &Path) {
        match sidebar::move_to_trash(path) {
            Ok(()) => {
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

/// 처음 실행할 때 만드는 안내 메모. samples/환영합니다.md 가 원본입니다 (그 파일을 고치면 여기도 바뀜).
/// 처음 실행할 때 만드는 안내 메모 (화면 언어로)
pub(super) fn welcome_note(lang: Lang) -> &'static str {
    match lang {
        Lang::Ko => include_str!("../../samples/welcome/ko.md"),
        Lang::En => include_str!("../../samples/welcome/en.md"),
        Lang::Zh => include_str!("../../samples/welcome/zh.md"),
        Lang::Ja => include_str!("../../samples/welcome/ja.md"),
    }
}

/// 안내 메모에 쓰는 예시 그림 (실행 파일에 들어 있음)
pub(super) const WELCOME_IMAGE: &[u8] = include_bytes!("../../samples/images/welcome.png");

/// `dir/시작하기.md` 와 예시 그림 `dir/images/welcome.png` 를 만듭니다.
pub(super) fn write_welcome_note(dir: &Path) -> std::io::Result<()> {
    let images = dir.join("images");
    std::fs::create_dir_all(&images)?;
    let img = images.join("welcome.png");
    if !img.exists() {
        std::fs::write(&img, WELCOME_IMAGE)?;
    }
    std::fs::write(dir.join(t!("시작하기.md")), welcome_note(crate::i18n::lang()))
}

/// 문서 첫 줄에서 제목을 뽑습니다 (파일 이름용)
pub(super) fn title_from_text(text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let cleaned: String = line.trim().trim_start_matches('#').replace(['*', '_', '`', '~', '[', ']'], "");
    let name = sanitize_file_name(cleaned.trim());
    (!name.is_empty()).then_some(name)
}

pub(super) fn doc_display_title(doc: &Document) -> String {
    match &doc.path {
        Some(p) if !doc.is_markdown() => doc.title(),
        Some(p) => p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| doc.title()),
        None => title_from_text(&doc.text).unwrap_or_else(|| doc.title()),
    }
}

pub(super) fn is_markdown_path(path: &Path) -> bool {
    has_extension(path, &["md", "markdown", "mdown", "mkd", "mdx"])
}

/// `dir/이름.ext`, 이미 있으면 `dir/이름 2.ext` …
pub(super) fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem} {n}.{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

pub(super) fn has_extension(path: &Path, exts: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e.to_lowercase().as_str()))
}

pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    a.canonicalize().map(|a| a == b).unwrap_or(false)
}

pub(super) fn sanitize_file_name(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| !r#"\/:*?"<>|"#.contains(*c) && !c.is_control()).take(60).collect();
    cleaned.trim().trim_end_matches('.').trim().to_owned()
}

pub(super) fn open_in_file_manager(dir: &Path) -> std::io::Result<()> {
    let cmd = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(cmd).arg(dir).spawn().map(|_| ())
}

/// 파일이면 탐색기에서 그 파일을 선택한 채로, 폴더면 그 폴더를 엽니다.
pub(super) fn reveal_in_file_manager(path: &Path) -> std::io::Result<()> {
    if cfg!(windows) && path.is_file() {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path.as_os_str());
        return std::process::Command::new("explorer").arg(arg).spawn().map(|_| ());
    }
    open_in_file_manager(if path.is_dir() { path } else { path.parent().unwrap_or(path) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_first_line() {
        assert_eq!(title_from_text("\n\n# 장보기: 우유/빵\n내용"), Some("장보기 우유빵".to_owned()));
        assert_eq!(title_from_text("   \n"), None);
        assert_eq!(title_from_text("**중요** 회의"), Some("중요 회의".to_owned()));
    }

    /// 안내 메모(samples/환영합니다.md)가 문서 모드에서 깨지지 않는지
    #[test]
    fn welcome_note_renders_every_feature() {
        let style = LiveStyle {
            size: 16.0,
            body: egui::FontFamily::Proportional,
            bold: egui::FontFamily::Proportional,
            mono: egui::FontFamily::Monospace,
            text: Color32::BLACK,
            muted: Color32::GRAY,
            heading: Color32::BLUE,
            code: Color32::RED,
            code_bg: Color32::LIGHT_GRAY,
            link: Color32::BLUE,
            quote: Color32::DARK_GRAY,
        };
        let size = |_: &str| Some(egui::vec2(480.0, 200.0));
        for lang in Lang::ALL {
            let note = welcome_note(lang);
            let out = live::layout(&live::LiveInput {
                text: note,
                style: &style,
                active_line: None,
                base_dir: Some(Path::new("C:/notes")),
                image_size: &size,
                max_image_width: 700.0,
            });
            assert_eq!(out.job.text, note);
            let has = |f: fn(&Deco) -> bool| out.decos.iter().any(f);
            assert!(has(|d| matches!(d, Deco::Image { align: ImgAlign::Center, .. })), "{lang:?} 가운데 정렬 그림");
            assert!(has(|d| matches!(d, Deco::Task { checked: true, .. })), "{lang:?}");
            assert!(has(|d| matches!(d, Deco::Task { checked: false, .. })), "{lang:?}");
            assert!(has(|d| matches!(d, Deco::Bullet { level: 1, .. })), "{lang:?} 하위 목록");
            assert!(has(|d| matches!(d, Deco::Quote { .. })), "{lang:?}");
            assert!(has(|d| matches!(d, Deco::Rule { .. })), "{lang:?}");
            assert!(has(|d| matches!(d, Deco::CodeBlock { .. })), "{lang:?}");
            assert!(has(|d| matches!(d, Deco::Table { .. })), "{lang:?}");
            assert!(has(|d| matches!(d, Deco::Link { .. })), "{lang:?}");
            // 모든 그림 경로는 실행 파일에 넣은 예시 그림 하나뿐
            assert!(out.image_uris.iter().all(|u| u.ends_with("images/welcome.png")), "{:?}", out.image_uris);
            // 안내 메모 속 "이 메모 안 제목으로 가기" 링크가 실제 제목을 가리키는지
            let jump = note.split("](#").nth(2).and_then(|s| s.split(')').next()).unwrap();
            let anchor = crate::links::Anchor::Heading(jump.to_owned());
            assert!(crate::links::anchor_range(note, &anchor).is_some(), "{lang:?}: #{jump}");
        }
    }
}
