//! 그림 넣기·붙여넣기, 그림 줄 선택·크기·위치, 그림 앞/뒤 커서 처리.

use super::*;

impl NotepadApp {
    /// 문서 폴더 밖의 그림이면 (설정에 따라) 문서 옆 그림 폴더로 복사해 그 복사본을 씁니다.
    pub(super) fn adopt_image(&self, image: &Path) -> PathBuf {
        let Some(dir) = self.doc().dir() else { return image.to_path_buf() };
        if !self.settings.keep_images_with_doc || image.starts_with(&dir) {
            return image.to_path_buf();
        }
        let target_dir = dir.join(&self.settings.image_folder);
        let name = image.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
        let ext = image.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "png".into());
        if std::fs::create_dir_all(&target_dir).is_err() {
            return image.to_path_buf();
        }
        let target = unique_path(&target_dir, &name, &ext);
        match std::fs::copy(image, &target) {
            Ok(_) => target,
            Err(_) => image.to_path_buf(),
        }
    }

    /// 문서에서 이미지 파일을 가리킬 경로 (가능하면 문서 기준 상대 경로)
    pub(super) fn image_link(&self, image: &Path) -> String {
        let image = &self.adopt_image(image);
        let name = image.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let target = match self.doc().dir() {
            Some(dir) => image.strip_prefix(&dir).map(Path::to_path_buf).unwrap_or_else(|_| image.to_path_buf()),
            None => image.to_path_buf(),
        };
        let mut s = target.to_string_lossy().replace('\\', "/");
        if s.contains([' ', '(', ')', '<', '>']) {
            s = format!("<{s}>");
        }
        format!("![{name}]({s})")
    }

    /// 그림은 한 줄을 다 차지하도록 앞뒤에 줄바꿈을 붙여 넣습니다.
    pub(super) fn insert_image_links(&mut self, links: Vec<String>) {
        if links.is_empty() || !self.doc().is_markdown() {
            return;
        }
        self.pending_action = Some(MdAction::Insert(format!("\n{}\n", links.join("\n"))));
        self.focus_editor = true;
    }

    pub(super) fn insert_image_dialog(&mut self) {
        if !self.doc().is_markdown() {
            return;
        }
        let mut dlg = rfd::FileDialog::new().add_filter("이미지", IMAGE_EXTENSIONS);
        if let Some(dir) = self.doc().dir() {
            dlg = dlg.set_directory(dir);
        }
        if let Some(paths) = dlg.pick_files() {
            let links = paths.iter().map(|p| self.image_link(p)).collect();
            self.insert_image_links(links);
        }
    }

    pub(super) fn paste_clipboard_image(&mut self, ctx: &egui::Context) {
        if !self.doc().is_markdown() {
            self.set_status(ctx, "텍스트 파일(.txt)에는 그림을 넣을 수 없습니다");
            return;
        }
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };
        let image = match clipboard.get_image() {
            Ok(img) => img,
            Err(_) => {
                // 탐색기에서 복사한 이미지 파일이면 그 파일을 가리키는 링크를 넣습니다.
                let files = clipboard.get().file_list().unwrap_or_default();
                let links: Vec<String> =
                    files.iter().filter(|p| has_extension(p, IMAGE_EXTENSIONS)).map(|p| self.image_link(p)).collect();
                if links.is_empty() {
                    self.set_status(ctx, "클립보드에 이미지가 없습니다");
                } else {
                    self.set_status(ctx, format!("이미지 {}개를 넣었습니다", links.len()));
                    self.insert_image_links(links);
                }
                return;
            }
        };
        let dir = match self.doc().dir() {
            Some(d) => d.join(&self.settings.image_folder),
            None => app_data_dir().join("images"),
        };
        let file = dir.join(format!("image_{}.png", chrono::Local::now().format("%Y%m%d_%H%M%S")));
        let result = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| {
            let buf = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())
                .ok_or_else(|| "이미지 형식이 올바르지 않습니다".to_owned())?;
            buf.save_with_format(&file, image::ImageFormat::Png).map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => {
                let link = self.image_link(&file);
                self.insert_image_links(vec![link]);
                self.set_status(ctx, "이미지를 붙여넣었습니다");
            }
            Err(err) => self.dialog = Some(Dialog::Message(format!("이미지를 저장하지 못했습니다.\n{err}"))),
        }
    }

    /// 커서가 그림 앞/뒤에 있을 때, TextEdit보다 먼저 입력을 살펴 그림 문법이 깨지지 않게 합니다.
    /// 커서가 그림 옆에 있으면 `true` (기본 커서 대신 그림 옆에 커서를 그림).
    pub(super) fn guard_image_edges(&mut self, ctx: &egui::Context, id: Id, spans: &[(usize, usize, usize)]) -> bool {
        let Some(mut state) = TextEditState::load(ctx, id) else { return false };
        let Some(range) = state.cursor.char_range() else { return false };
        if !range.is_empty() {
            return false;
        }
        let cur = range.primary.index.0;
        let Some(&(start, end, line_byte)) = spans.iter().find(|(s0, e0, _)| cur == *s0 || cur == *e0) else {
            return false;
        };
        let at_start = cur == start;
        let (typing, backspace, delete) = ctx.input(|i| {
            let typing = i.events.iter().any(|e| match e {
                egui::Event::Text(t) => !t.is_empty(),
                egui::Event::Paste(t) => !t.is_empty(),
                egui::Event::Ime(egui::ImeEvent::Preedit { text: t, .. })
                | egui::Event::Ime(egui::ImeEvent::Commit(t)) => !t.is_empty(),
                egui::Event::Key { key: Key::Tab, pressed: true, .. } => true,
                _ => false,
            });
            let key =
                |k: Key| i.events.iter().any(|e| matches!(e, egui::Event::Key { key, pressed: true, .. } if *key == k));
            (typing, key(Key::Backspace), key(Key::Delete))
        });
        let drop_key =
            |k: Key| ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key, .. } if *key == k)));
        let doc = &mut self.docs[self.active];
        let mut move_to = None;

        if typing {
            // 그림 앞이면 위에, 뒤면 아래에 새 줄을 만들고 거기에 입력
            if at_start {
                doc.text.insert(markdown::char_to_byte(&doc.text, start), '\n');
                move_to = Some(start);
            } else {
                doc.text.insert(markdown::char_to_byte(&doc.text, end), '\n');
                move_to = Some(end + 1);
            }
            doc.mark_changed();
        } else if backspace && !at_start {
            // 그림 뒤에서 Backspace: 먼저 그림을 선택 (한 번 더 누르면 지움)
            drop_key(Key::Backspace);
            self.selected_image = Some((doc.id, line_byte));
            ctx.memory_mut(|m| m.surrender_focus(id));
        } else if delete && at_start {
            drop_key(Key::Delete);
            self.selected_image = Some((doc.id, line_byte));
            ctx.memory_mut(|m| m.surrender_focus(id));
        } else if backspace && at_start {
            // 앞 줄이 빈 줄이면 그 줄만 지워지므로 그대로, 아니면 지우지 않고 앞 줄 끝으로
            let b = line_byte;
            let prev_empty = b >= 1 && (b == 1 || doc.text.as_bytes()[b - 2] == b'\n');
            if b == 0 {
                drop_key(Key::Backspace);
            } else if !prev_empty {
                drop_key(Key::Backspace);
                move_to = Some(start - 1);
            }
        } else if delete && !at_start {
            // 다음 줄이 빈 줄이거나 끝이면 그대로, 아니면 다음 줄 처음으로
            let b = markdown::char_to_byte(&doc.text, end);
            let next_empty = b + 1 >= doc.text.len() || doc.text.as_bytes()[b + 1] == b'\n';
            if !next_empty {
                drop_key(Key::Delete);
                move_to = Some(end + 1);
            }
        }
        if let Some(c) = move_to {
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            state.store(ctx, id);
            return false;
        }
        self.selected_image.is_none_or(|(d, _)| d != self.docs[self.active].id)
    }

    pub(super) fn apply_image_action(&mut self, action: ImageAction) {
        let doc = &mut self.docs[self.active];
        let (line, change) = match action {
            ImageAction::Select(_) => return,
            ImageAction::Width(l, w) => (l, Some((Some(w), None))),
            ImageAction::Align(l, a) => (l, Some((None, Some(a)))),
            ImageAction::Delete(l) => (l, None),
        };
        if image_line_at(&doc.text, line).is_none() {
            return;
        }
        let end = doc.text[line..].find('\n').map_or(doc.text.len(), |i| line + i);
        let body = doc.text[line..end].to_owned();
        let indent = &body[..body.len() - body.trim_start().len()];
        let Some(img) = live::parse_image_line(body.trim_start()) else { return };
        match change {
            Some((width, align)) => {
                let width = width.unwrap_or(img.width);
                let align = align.unwrap_or(img.align);
                let new_line = format!("{indent}{}", live::format_image_line(img.alt, img.dest_raw, width, align));
                if new_line != body {
                    doc.text.replace_range(line..end, &new_line);
                    doc.mark_changed();
                }
            }
            None => {
                // 줄바꿈까지 지우기
                let end = if end < doc.text.len() { end + 1 } else { end };
                doc.text.replace_range(line..end, "");
                doc.mark_changed();
                self.selected_image = None;
                self.focus_editor = true;
            }
        }
    }
}

/// 그림에 대한 동작 (그림 줄이 시작하는 바이트 위치로 가리킴)
pub(super) enum ImageAction {
    Select(usize),
    /// 본문 폭 대비 %, `None`이면 원래 크기
    Width(usize, Option<u32>),
    Align(usize, ImgAlign),
    Delete(usize),
}

/// 문서 모드에서 그림 줄들의 (시작 글자, 끝 글자, 시작 바이트)
pub(super) fn image_spans(doc: &Document) -> Vec<(usize, usize, usize)> {
    let Some((_, _, decos, _)) = &doc.live_cache else { return Vec::new() };
    decos
        .iter()
        .filter_map(|d| match d {
            Deco::Image { at, end, line, .. } => Some((*at, *end, line.start)),
            _ => None,
        })
        .collect()
}

/// `line` 위치에서 시작하는 줄이 그림 줄이면 (경로, 너비, 위치)
pub(super) fn image_line_at(text: &str, line: usize) -> Option<(String, Option<u32>, ImgAlign)> {
    if line > text.len() || !text.is_char_boundary(line) || (line > 0 && text.as_bytes()[line - 1] != b'\n') {
        return None;
    }
    let end = text[line..].find('\n').map_or(text.len(), |i| line + i);
    let img = live::parse_image_line(text[line..end].trim_start())?;
    Some((img.dest.to_owned(), img.width, img.align))
}

/// 선택한 그림 위에 뜨는 도구 막대: 크기 · 위치 · 지우기.
/// 평소에는 그림 바로 위에 뜨고, 그 자리가 편집 영역 위로 벗어나면 편집 영역 맨 위(서식 도구 바로 아래)에 붙습니다.
pub(super) fn image_toolbar_ui(
    ctx: &egui::Context,
    img: egui::Rect,
    editor_top: f32,
    line: usize,
    align: ImgAlign,
    width: Option<u32>,
) -> Option<ImageAction> {
    let mut action = None;
    let h = 44.0;
    let pos = egui::pos2(img.left(), (img.top() - h - 6.0).max(editor_top + 6.0));
    egui::Area::new(Id::new("image_toolbar")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        Frame::popup(ui.style()).inner_margin(Margin::same(5)).corner_radius(10).show(ui, |ui| {
            let base = ui.visuals().window_fill;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for pct in [25, 50, 75, 100] {
                    let label = RichText::new(format!("{pct}%")).size(12.5);
                    if ui_kit::text_button(ui, label, &format!("본문 폭의 {pct}%"), width == Some(pct), base).clicked()
                    {
                        action = Some(ImageAction::Width(line, Some(pct)));
                    }
                }
                if ui_kit::text_button(
                    ui,
                    RichText::new("원래 크기").size(12.5),
                    "그림 원래 크기 (본문 폭을 넘지 않게)",
                    width.is_none(),
                    base,
                )
                .clicked()
                {
                    action = Some(ImageAction::Width(line, None));
                }
                ui.add_space(8.0);
                for (a, icon, tip) in [
                    (ImgAlign::Left, Icon::AlignLeft, "왼쪽"),
                    (ImgAlign::Center, Icon::AlignCenter, "가운데"),
                    (ImgAlign::Right, Icon::AlignRight, "오른쪽"),
                ] {
                    if ui_kit::icon_button(ui, icon, tip, align == a, base).clicked() {
                        action = Some(ImageAction::Align(line, a));
                    }
                }
                ui.add_space(8.0);
                if ui_kit::icon_button(ui, Icon::Trash, "그림 지우기 (Delete)", false, base).clicked() {
                    action = Some(ImageAction::Delete(line));
                }
            });
        });
    });
    action
}

/// 불러온 그림의 크기 (아직이거나 실패하면 `None`)
pub(super) fn image_size(ctx: &egui::Context, uri: &str) -> Option<egui::Vec2> {
    match ctx.try_load_texture(uri, egui::TextureOptions::LINEAR, egui::SizeHint::default()) {
        Ok(egui::load::TexturePoll::Ready { texture }) => Some(texture.size),
        _ => None,
    }
}
