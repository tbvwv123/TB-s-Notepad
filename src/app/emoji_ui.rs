//! 커스텀 이모지 (`src/emoji.rs`) 를 편집기에서: 넣기, 등록하기, 한 덩어리로 지우기.
//! 고르는 목록은 `/` 명령 목록과 같은 창을 씁니다 (`slash.rs`).

use super::*;
use crate::emoji::{self, Emoji};

impl NotepadApp {
    /// 지금 문서의 이모지 폴더: 문서가 든 메모 폴더의 `.emoji`, 아니면 첫 메모 폴더의 것
    pub(super) fn emoji_dir(&self) -> Option<PathBuf> {
        let roots = &self.settings.roots;
        let root = self
            .doc()
            .path
            .as_ref()
            .and_then(|p| roots.iter().find(|r| p.starts_with(r)))
            .or_else(|| roots.iter().find(|r| r.exists()))?;
        Some(emoji::dir_for(root))
    }

    pub(super) fn emoji_list(&self) -> Vec<Emoji> {
        self.emoji_dir().map(|d| emoji::list(&d)).unwrap_or_default()
    }

    /// 지금 문서에 넣을 이모지 문법 (가능하면 문서 기준 상대 경로)
    fn emoji_markdown(&self, e: &Emoji) -> String {
        let dest = match self.doc().dir().and_then(|d| links::relative_path(&d, &e.path)) {
            Some(rel) => links::link_dest(&rel, None),
            None => links::link_dest(&e.path, None),
        };
        emoji::markdown(&e.name, &dest)
    }

    /// 지금 문서의 `range`(글자) 를 이모지로 바꾸고 커서를 그 뒤로
    pub(super) fn insert_emoji(&mut self, ctx: &egui::Context, id: Id, range: Range<usize>, e: &Emoji) {
        let md = self.emoji_markdown(e);
        let doc = &mut self.docs[self.active];
        let (a, b) = (markdown::char_to_byte(&doc.text, range.start), markdown::char_to_byte(&doc.text, range.end));
        doc.text.replace_range(a..b, &md);
        doc.mark_changed();
        let end = range.start + md.chars().count();
        let mut state = TextEditState::load(ctx, id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(end))));
        state.store(ctx, id);
        self.focus_editor = true;
        ctx.request_repaint();
    }

    /// 그림 파일을 골라 새 이모지 이름을 묻습니다. 등록하면 `insert_at`(글자 위치) 에 넣습니다.
    pub(super) fn add_emoji_dialog(&mut self, name: &str, insert_at: Option<usize>) {
        let Some(dir) = self.emoji_dir() else {
            self.dialog = Some(Dialog::Message(
                t!("이모지를 둘 메모 폴더가 없습니다. 사이드바에서 메모 폴더를 먼저 추가하세요.").into(),
            ));
            return;
        };
        let Some(source) = rfd::FileDialog::new().add_filter(t!("그림"), emoji::EXTENSIONS).pick_file() else {
            return;
        };
        let mut name = emoji::clean_name(name);
        if name.is_empty() {
            name = emoji::clean_name(&source.file_stem().unwrap_or_default().to_string_lossy());
        }
        let insert_at = insert_at.map(|at| (self.doc().id, at));
        self.dialog = Some(Dialog::RegisterEmoji { source, dir, name, insert_at });
    }

    /// 본문의 그림을 이모지로 (그림 오른쪽 클릭 메뉴)
    pub(super) fn register_emoji_from_image(&mut self, line: usize) {
        let Some((dest, _, _)) = image_line_at(&self.doc().text, line) else { return };
        let path = PathBuf::from(markdown::percent_decode(&dest));
        let path = match self.doc().dir() {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path,
        };
        if dest.contains("://") || !path.is_file() {
            self.dialog = Some(Dialog::Message(t!("이 PC에 있는 그림만 이모지로 등록할 수 있습니다.").into()));
            return;
        }
        let Some(dir) = self.emoji_dir() else {
            self.dialog = Some(Dialog::Message(
                t!("이모지를 둘 메모 폴더가 없습니다. 사이드바에서 메모 폴더를 먼저 추가하세요.").into(),
            ));
            return;
        };
        let name = emoji::clean_name(&path.file_stem().unwrap_or_default().to_string_lossy());
        self.dialog = Some(Dialog::RegisterEmoji { source: path, dir, name, insert_at: None });
    }

    /// 등록 창에서 "추가"를 누름
    pub(super) fn finish_register_emoji(
        &mut self,
        ctx: &egui::Context,
        source: &Path,
        dir: &Path,
        name: &str,
        insert_at: Option<(u64, usize)>,
    ) {
        let name = emoji::clean_name(name);
        if name.is_empty() {
            return;
        }
        match emoji::register(dir, source, &name) {
            Ok(e) => {
                self.set_status(ctx, tf!(":{}: 이모지를 추가했습니다", e.name));
                if let Some((doc_id, at)) = insert_at
                    && self.doc().id == doc_id
                {
                    let at = at.min(self.doc().text.chars().count());
                    self.insert_emoji(ctx, editor_id(doc_id), at..at, &e);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                self.dialog = Some(Dialog::Message(tf!(":{}: 이모지가 이미 있습니다. 다른 이름을 써 주세요.", name)));
            }
            Err(err) => self.dialog = Some(Dialog::Message(tf!("이모지를 추가하지 못했습니다.\n{}", err))),
        }
    }

    /// 이모지는 한 덩어리: 바로 뒤에서 Backspace, 바로 앞에서 Delete 를 누르면 통째로 지웁니다
    /// (한 글자씩 지우면 숨은 문법이 깨져 드러나므로).
    pub(super) fn guard_emoji_edges(&mut self, ctx: &egui::Context, id: Id, spans: &[(usize, usize)]) {
        let Some(mut state) = TextEditState::load(ctx, id) else { return };
        let Some(range) = state.cursor.char_range() else { return };
        if !range.is_empty() {
            return;
        }
        let p = range.primary.index.0;
        let (backspace, delete) = ctx.input(|i| {
            let key = |k: Key| {
                i.events.iter().any(|e| {
                    matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                        if *key == k && !modifiers.command && !modifiers.alt)
                })
            };
            (key(Key::Backspace), key(Key::Delete))
        });
        let Some(&(s0, e0)) = spans.iter().find(|(s0, e0)| (backspace && *e0 == p) || (delete && *s0 == p)) else {
            return;
        };
        let doc = &mut self.docs[self.active];
        let (a, b) = (markdown::char_to_byte(&doc.text, s0), markdown::char_to_byte(&doc.text, e0));
        // 배치한 뒤 글이 바뀌었으면 (이 프레임의 다른 편집) 건드리지 않음
        if emoji::parse_inline(&doc.text[a..]).map(|e| e.len) != Some(b - a) {
            return;
        }
        doc.text.replace_range(a..b, "");
        doc.mark_changed();
        let k = if backspace { Key::Backspace } else { Key::Delete };
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key, .. } if *key == k)));
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(s0))));
        state.store(ctx, id);
    }
}
