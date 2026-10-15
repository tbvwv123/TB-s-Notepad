//! 본문 편집기: TextEdit 배치(문서 모드 / 원본 보기), 꾸밈·찾은 위치·줄 번호 그리기.

use super::decorations::{DecoView, paint_decorations};
use super::*;

/// 글자를 배치할 때 필요한 값 (layouter 안에서 씀)
struct LayoutParams<'a> {
    doc_mode: bool,
    wrap: bool,
    active_line: Option<usize>,
    image_state: u64,
    base_dir: Option<&'a Path>,
    live_style: &'a LiveStyle,
    source_style: &'a HighlightStyle,
    ctx: &'a egui::Context,
}

impl NotepadApp {
    /// 본문 열 위치: (왼쪽 여백, 열 너비)
    pub(super) fn column(&self, avail: f32, wrap: bool) -> (f32, f32) {
        let max = self.settings.max_text_width;
        if !wrap || max <= 0.0 || avail <= max {
            return (0.0, avail);
        }
        (((avail - max) / 2.0).floor(), max)
    }

    pub(super) fn editor(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let palette = self.settings.palette();
        let s = self.settings.clone();
        let doc_mode = self.is_document_mode();
        let wrap = s.word_wrap || doc_mode;
        let highlight_line = s.highlight_current_line && !doc_mode;
        let source_font = FontId::new(s.editor_font_size, editor_family());
        let source_style = HighlightStyle::new(
            &palette,
            source_font.clone(),
            s.line_spacing,
            s.markdown_highlight && self.doc().is_markdown(),
        );
        let live_style = live_style(&palette, s.editor_font_size);
        let fake_bold = self.fonts.fake_bold;
        let id = editor_id(self.doc().id);
        let (side, col_w) = self.column(ui.available_width(), wrap);

        // 본문 양옆 빈 곳을 눌러도 편집기로 (TextEdit보다 먼저 등록해야 TextEdit 클릭을 가로채지 않음)
        if ui.interact(ui.max_rect(), id.with("background"), egui::Sense::click()).clicked() {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        self.apply_pending_edits(&ctx, id);
        // 복사해 둔 메모 링크를 붙여넣으면 이 메모 기준 상대 경로로
        if ctx.memory(|m| m.has_focus(id)) {
            self.relativize_paste(&ctx);
        }
        // / 명령 목록이 열려 있으면 ↑↓ · Enter · Esc 는 목록이 받음
        let slash_key = if ctx.memory(|m| m.has_focus(id)) { self.slash_keys(&ctx, id) } else { None };
        let typed_slash = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t.ends_with('/'))));

        let find_matches = if self.find.open && !self.find.query.is_empty() {
            self.refresh_matches();
            self.find.matches.clone()
        } else {
            Vec::new()
        };
        let find_current = self.find.current.clone();

        // 문서 모드: 기호를 드러낼 줄(커서 줄)과 그림 불러오기 상태
        let has_focus = ctx.memory(|m| m.has_focus(id));
        // Ctrl 을 누르고 있는 동안은 기호를 드러낼 줄을 바꾸지 않습니다:
        // Ctrl+클릭으로 커서가 링크 줄로 옮겨 가며 기호가 드러나면 링크 자리가 밀려 클릭이 빗나가기 때문.
        let doc_id = self.docs[self.active].id;
        let frozen = self.revealed_line.filter(|(d, _)| *d == doc_id).map(|(_, l)| l);
        let active_line = match frozen {
            Some(l) if ctx.input(|i| i.modifiers.command) => l,
            _ if doc_mode && has_focus => self.cursor_line(&ctx, id),
            _ => None,
        };
        self.revealed_line = Some((doc_id, active_line));
        let image_state = self.image_load_state(&ctx);

        // 그림 줄은 한 덩어리: 커서는 그림 앞/뒤에만, 그 자리에서 입력하면 새 줄을 만들어 그림 문법이 깨지지 않게
        let spans = image_spans(&self.docs[self.active]);
        let hide_caret = doc_mode && has_focus && self.guard_image_edges(&ctx, id, &spans);

        let last_cursor = self.last_cursor.filter(|(d, _)| *d == doc_id).map(|(_, c)| c);
        let selected_img = self.selected_image.filter(|(d, _)| *d == doc_id).map(|(_, l)| l);

        let bg = c(palette.editor_bg);
        let text_color = c(palette.text);
        let accent = c(palette.accent);
        let number_font = FontId::new((s.editor_font_size * 0.75).max(8.0), egui::FontFamily::Monospace);
        let row_h = s.editor_font_size * if doc_mode { 1.35 } else { s.line_spacing };

        let doc = &mut self.docs[self.active];
        let base_dir = doc.dir();
        let gutter_w = if s.show_line_numbers {
            let digits = (doc.text.matches('\n').count() + 1).to_string().len().max(2);
            digits as f32 * ctx.fonts_mut(|f| f.glyph_width(&number_font, '0')) + 22.0
        } else {
            0.0
        };

        let mut scroll_to_cursor = std::mem::take(&mut self.scroll_to_cursor);
        let mut cursor_info = CursorInfo::default();
        let mut new_cursor = None;
        let mut editor_focused = false;
        let mut decorations = decorations::DecoOutput::default();
        // 편집 영역(스크롤되는 본문)의 맨 위: 그림 도구 막대가 서식 도구와 겹치지 않게
        let mut editor_top = f32::NEG_INFINITY;
        // 화면에서 커서 자리 (/ 명령 목록을 그 아래에 띄움)
        let mut caret_rect = None;

        let scroll = if wrap { ScrollArea::vertical() } else { ScrollArea::both() };
        scroll.id_salt(("editor_scroll", doc.id)).auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(side);
                let left = ui.cursor().left();
                let bg_shape = ui.painter().add(Shape::Noop);
                ui.add_space(gutter_w);

                let params = LayoutParams {
                    doc_mode,
                    wrap,
                    active_line,
                    image_state,
                    base_dir: base_dir.as_deref(),
                    live_style: &live_style,
                    source_style: &source_style,
                    ctx: &ctx,
                };
                let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
                    let job =
                        layout_job(buf.as_str(), wrap_width, &params, &mut doc.live_cache, &mut doc.highlight_cache);
                    ui.fonts_mut(|f| f.layout_job(job))
                };

                let rows = ((ui.available_height() - 82.0) / row_h).max(1.0) as usize;
                if hide_caret {
                    // 기본 커서는 그림 줄 맨 위 구석에 그려지므로 감추고, 그림 옆에 따로 그립니다.
                    ui.visuals_mut().text_cursor.stroke.color = Color32::TRANSPARENT;
                }
                let output = TextEdit::multiline(&mut doc.text)
                    .id(id)
                    .font(if doc_mode { FontId::new(s.editor_font_size, doc_family()) } else { source_font.clone() })
                    // 프레임을 직접 주면 egui는 .margin() 대신 프레임의 안쪽 여백을 쓰므로 둘 다 지정
                    .frame(Frame::NONE.inner_margin(EDITOR_MARGIN))
                    .margin(EDITOR_MARGIN)
                    .desired_width(if wrap { col_w - gutter_w } else { f32::INFINITY })
                    .desired_rows(rows)
                    .lock_focus(true)
                    .hint_text(if doc_mode { "제목을 쓰고 Enter를 눌러 시작하세요" } else { "" })
                    .layouter(&mut layouter)
                    .show(ui);

                let response = &output.response.response;
                if response.changed() {
                    doc.mark_changed();
                }
                editor_focused = response.has_focus();

                let galley = &output.galley;
                let origin = (output.galley_pos - egui::vec2(galley.rect.left(), 0.0)).to_vec2();
                let full = response.rect;
                let clip = ui.clip_rect();
                editor_top = clip.top();
                let text_left = output.galley_pos.x;
                let text_right = full.right() - EDITOR_MARGIN.right as f32;
                let pos = |ci: usize| galley.pos_from_cursor(CCursor::new(ci)).translate(origin);

                // 문서 모드 꾸밈 (목록 점, 체크박스, 인용 막대, 구분선, 그림, 코드 블록 배경)
                if doc_mode && let Some((_, _, decos, _)) = &doc.live_cache {
                    let view = DecoView {
                        galley,
                        origin,
                        clip,
                        text_left,
                        text_right,
                        font_size: s.editor_font_size,
                        editor_id: id,
                        selected_image: selected_img,
                        clicked_at: if response.clicked() { ui.input(|i| i.pointer.interact_pos()) } else { None },
                        bg,
                        text: text_color,
                        muted: c(palette.muted),
                        accent,
                        media: &self.media,
                        fake_bold,
                    };
                    decorations = paint_decorations(ui, &view, decos);
                }
                let mut bg_shapes = std::mem::take(&mut decorations.bg);
                let mut fg_shapes = std::mem::take(&mut decorations.fg);
                // 고른 글꼴에 굵은 글꼴이 없으면 굵은 글씨(제목, **굵게**)만 오른쪽으로 겹쳐 그려 굵게 보이게
                if doc_mode
                    && fake_bold
                    && let Some(job) = crate::fonts::bold_only_job(&galley.job)
                {
                    let bold_galley = ui.fonts_mut(|f| f.layout_job(job));
                    let ppp = ctx.pixels_per_point();
                    for dx in crate::fonts::fake_bold_offsets(s.editor_font_size * 1.2, ppp) {
                        let at = output.galley_pos + egui::vec2(dx, 0.0);
                        fg_shapes.insert(0, Shape::galley(at, bold_galley.clone(), Color32::TRANSPARENT));
                    }
                }
                if s.show_line_numbers {
                    let bottom = full.bottom().max(clip.bottom());
                    let gutter =
                        egui::Rect::from_min_max(egui::pos2(left, full.top()), egui::pos2(left + gutter_w, bottom));
                    bg_shapes.insert(0, Shape::rect_filled(gutter, 0.0, mix(bg, text_color, 0.025)));
                }

                // 커서가 그림 줄의 경로 글자 안으로 들어가면 그림 앞/뒤로 밀어냅니다.
                let mut cursor_range = output.cursor_range.or_else(|| output.state.cursor.char_range());
                if doc_mode
                    && editor_focused
                    && let Some(range) = cursor_range
                {
                    let vertical = ui.input(|i| {
                        [Key::ArrowUp, Key::ArrowDown, Key::PageUp, Key::PageDown].into_iter().any(|k| i.key_pressed(k))
                    });
                    if let Some(snapped) = snap_out_of_images(range, &decorations.image_rects, last_cursor, vertical) {
                        let mut st = output.state.clone();
                        st.cursor.set_char_range(Some(snapped));
                        st.store(&ctx, id);
                        cursor_range = Some(snapped);
                        ctx.request_repaint();
                    }
                }

                if let Some(range) = cursor_range {
                    new_cursor = Some(range.primary.index.0);
                    let rect = pos(range.primary.index.0);
                    caret_rect = Some(rect);
                    // 그림 앞/뒤에 있는 커서는 그림 높이만큼 세로선으로
                    if doc_mode && editor_focused && range.is_empty() {
                        let p = range.primary.index.0;
                        let beside = decorations.image_rects.iter().find(|(s0, e0, _)| p == *s0 || p == *e0);
                        if let Some(&(s0, _, r)) = beside {
                            let x = if p == s0 { r.left() - 5.0 } else { r.right() + 5.0 };
                            fg_shapes.push(Shape::line_segment(
                                [egui::pos2(x, r.top()), egui::pos2(x, r.bottom())],
                                Stroke::new(2.0, accent),
                            ));
                        }
                        // 이번 프레임에 기본 커서를 감출지 잘못 짐작했으면 한 번 더 그림
                        if beside.is_some() != hide_caret {
                            ctx.request_repaint();
                        }
                    }
                    if highlight_line && range.is_empty() && editor_focused {
                        let line = egui::Rect::from_x_y_ranges(left..=full.right().max(left + col_w), rect.y_range());
                        bg_shapes.push(Shape::rect_filled(line, 0.0, mix(bg, text_color, 0.05)));
                    }
                    if scroll_to_cursor {
                        ui.scroll_to_rect(rect.expand(row_h * 2.0), Some(Align::Center));
                        scroll_to_cursor = false;
                    }
                    cursor_info = cursor_info_at(&doc.text, range);
                }

                // 찾은 위치 (현재 위치는 더 진하게)
                let selected = cursor_range.map(|r| {
                    let r = r.as_sorted_char_range();
                    r.start.0..r.end.0
                });
                let current = find_current.as_ref().or(selected.as_ref());
                bg_shapes.extend(find_match_shapes(&pos, &find_matches, current, clip, full.right(), palette.accent));

                ui.painter().set(bg_shape, Shape::Vec(bg_shapes));
                ui.painter().extend(fg_shapes);

                if s.show_line_numbers {
                    let color = mix(bg, c(palette.muted), 0.75);
                    paint_line_numbers(
                        ui.painter(),
                        galley,
                        origin,
                        clip,
                        left + gutter_w - 12.0,
                        row_h,
                        &number_font,
                        color,
                    );
                }
            });
        });

        self.slash_menu(&ctx, id, typed_slash, caret_rect, editor_focused, slash_key);

        // 그림 도구 막대 (선택한 그림 위에 떠 있음)
        let mut image_action = decorations.image_action;
        if let Some((r, line, align, kind)) = decorations.image_toolbar {
            let current = image_line_at(&self.docs[self.active].text, line).and_then(|(_, w, _)| w);
            if let Some(a) = image_toolbar_ui(&ctx, r, editor_top, line, align, current, kind) {
                image_action = Some(a);
            }
        }
        match image_action {
            Some(ImageAction::Select(line)) => {
                self.selected_image = Some((doc_id, line));
                ctx.memory_mut(|m| m.surrender_focus(id));
                editor_focused = false;
            }
            Some(a) => self.apply_image_action(a),
            None if editor_focused => self.selected_image = None,
            None => {}
        }

        for path in decorations.media_requests {
            self.media.request(&ctx, path);
        }
        if let Some(a) = decorations.media_action {
            self.apply_media_action(&ctx, a);
        }
        if let Some(url) = decorations.follow_link {
            self.follow_link(&ctx, &url);
        }
        if let Some((byte, checked)) = decorations.toggle_task {
            let doc = &mut self.docs[self.active];
            if matches!(doc.text.as_bytes().get(byte), Some(b' ' | b'x' | b'X')) {
                doc.text.replace_range(byte..byte + 1, if checked { "x" } else { " " });
                doc.mark_changed();
            }
        }
        self.cursor = cursor_info;
        if let Some(c) = new_cursor {
            self.last_cursor = Some((doc_id, c));
        }
        self.scroll_to_cursor = scroll_to_cursor;
        // 편집기로 돌아와 커서를 움직이면 다음 찾기는 그 커서에서 시작
        if editor_focused {
            self.find.current = None;
        }
    }

    /// 서식 도구·찾기·그림 선택에서 예약한 일을 TextEdit을 그리기 전에 처리합니다.
    fn apply_pending_edits(&mut self, ctx: &egui::Context, id: Id) {
        if let Some(action) = self.pending_action.take()
            && (self.doc().is_markdown() || matches!(action, MdAction::Insert(_)))
        {
            let doc = &mut self.docs[self.active];
            let mut state = TextEditState::load(ctx, id).unwrap_or_default();
            let end = doc.text.chars().count();
            let sel = state
                .cursor
                .char_range()
                .map(|r| {
                    let r = r.as_sorted_char_range();
                    r.start.0.min(end)..r.end.0.min(end)
                })
                .unwrap_or(end..end);
            let new_sel = markdown::apply_action(&mut doc.text, sel, &action);
            doc.mark_changed();
            state
                .cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(new_sel.start), CCursor::new(new_sel.end))));
            state.store(ctx, id);
            self.focus_editor = true;
            self.scroll_to_cursor = true;
        }
        if let Some(sel) = self.pending_select.take() {
            let mut state = TextEditState::load(ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.start), CCursor::new(sel.end))));
            state.store(ctx, id);
            self.scroll_to_cursor = true;
            // 찾기 카드의 "n/전체" 표시가 바로 갱신되도록
            ctx.request_repaint();
        }
        // 사이드바 등을 누른 바로 그 프레임에는 egui가 포커스를 다시 거둘 수 있어서,
        // 편집기가 실제로 포커스를 받을 때까지 몇 프레임 동안 요청합니다.
        if self.focus_editor && self.dialog.is_none() {
            if ctx.memory(|m| m.has_focus(id)) || self.focus_attempts >= 5 {
                self.focus_editor = false;
                self.focus_attempts = 0;
            } else {
                ctx.memory_mut(|m| m.request_focus(id));
                self.focus_attempts += 1;
                ctx.request_repaint();
            }
        }

        // 선택한 그림: Delete/Backspace 로 지우기, Esc 로 선택 해제
        if let Some((doc_id, line)) = self.selected_image {
            let valid = doc_id == self.doc().id && image_line_at(&self.doc().text, line).is_some();
            let typing_elsewhere = ctx.memory(|m| m.focused().is_some());
            if !valid {
                self.selected_image = None;
            } else if !typing_elsewhere && self.dialog.is_none() {
                let del = ctx.input_mut(|i| {
                    i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace)
                });
                if del {
                    self.apply_image_action(ImageAction::Delete(line));
                } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                    self.selected_image = None;
                    self.focus_editor = true;
                }
            }
        }
    }

    /// 커서가 있는 줄 번호 (0부터)
    fn cursor_line(&self, ctx: &egui::Context, id: Id) -> Option<usize> {
        let range = TextEditState::load(ctx, id)?.cursor.char_range()?;
        let text = &self.doc().text;
        let b = markdown::char_to_byte(text, range.primary.index.0);
        Some(text[..b].matches('\n').count())
    }

    /// 이 문서 그림들의 불러오기 상태. 그림이 다 불러와지면 값이 바뀌어 다시 배치합니다.
    fn image_load_state(&self, ctx: &egui::Context) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Some((_, _, _, uris)) = &self.doc().live_cache {
            for u in uris {
                image_size(ctx, u).map(|v| (v.x as i32, v.y as i32)).hash(&mut h);
            }
        }
        h.finish()
    }
}

fn live_style(palette: &Palette, size: f32) -> LiveStyle {
    let bg = c(palette.editor_bg);
    let text = c(palette.text);
    LiveStyle {
        size,
        body: doc_family(),
        bold: bold_family(),
        mono: egui::FontFamily::Monospace,
        text,
        muted: c(palette.muted),
        heading: c(palette.heading),
        code: c(palette.code),
        code_bg: mix(bg, text, 0.06),
        link: c(palette.link),
        quote: c(palette.quote),
    }
}

/// 글자 배치 (결과는 문서에 캐시). 문서 모드는 기호를 숨긴 서식, 원본 보기는 문법 강조.
fn layout_job(
    text: &str,
    wrap_width: f32,
    p: &LayoutParams<'_>,
    live_cache: &mut Option<(u64, egui::text::LayoutJob, Vec<Deco>, Vec<String>)>,
    highlight_cache: &mut Option<(u64, egui::text::LayoutJob)>,
) -> egui::text::LayoutJob {
    let mut job = if p.doc_mode {
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut h);
            p.active_line.hash(&mut h);
            p.live_style.hash_into(&mut h);
            (wrap_width as i32).hash(&mut h);
            p.image_state.hash(&mut h);
            p.base_dir.hash(&mut h);
            h.finish()
        };
        match live_cache {
            Some((k, job, _, _)) if *k == key => job.clone(),
            _ => {
                let size_of = |uri: &str| image_size(p.ctx, uri);
                let out = live::layout(&live::LiveInput {
                    text,
                    style: p.live_style,
                    active_line: p.active_line,
                    base_dir: p.base_dir,
                    image_size: &size_of,
                    max_image_width: wrap_width - 4.0,
                });
                *live_cache = Some((key, out.job.clone(), out.decos, out.image_uris));
                out.job
            }
        }
    } else {
        let key = p.source_style.cache_key(text);
        match highlight_cache {
            Some((k, job)) if *k == key => job.clone(),
            _ => {
                let job = markdown::highlight(text, p.source_style);
                *highlight_cache = Some((key, job.clone()));
                job
            }
        }
    };
    job.wrap.max_width = if p.wrap { wrap_width } else { f32::INFINITY };
    job
}

/// 커서가 그림 줄의 경로 안에 있으면 옮길 자리.
/// 위/아래로 들어오면 그림 앞, 그림 앞에서 → 는 그림 뒤, 그림 뒤에서 ← 는 그림 앞.
fn snap_out_of_images(
    range: CCursorRange,
    image_rects: &[(usize, usize, egui::Rect)],
    last_cursor: Option<usize>,
    vertical: bool,
) -> Option<CCursorRange> {
    let p = range.primary.index.0;
    let target = image_rects.iter().find_map(|&(s0, e0, _)| {
        if s0 < p && p < e0 {
            Some(if vertical || last_cursor != Some(s0) { s0 } else { e0 })
        } else if p == e0 && vertical && last_cursor != Some(e0) {
            Some(s0)
        } else {
            None
        }
    })?;
    Some(if range.is_empty() {
        CCursorRange::one(CCursor::new(target))
    } else {
        CCursorRange { primary: CCursor::new(target), ..range }
    })
}

/// 상태 표시줄용 줄·열·선택 글자 수
fn cursor_info_at(text: &str, range: CCursorRange) -> CursorInfo {
    let r = range.as_sorted_char_range();
    let p = markdown::char_to_byte(text, range.primary.index.0);
    let before = &text[..p];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    CursorInfo {
        line: before.matches('\n').count() + 1,
        column: before[line_start..].chars().count() + 1,
        selected: r.end.0 - r.start.0,
    }
}

/// 찾은 위치마다 반투명 배경 (보이는 부분만, 최대 5000개)
fn find_match_shapes(
    pos: &dyn Fn(usize) -> egui::Rect,
    matches: &[Range<usize>],
    current: Option<&Range<usize>>,
    clip: egui::Rect,
    right: f32,
    accent: [u8; 3],
) -> Vec<Shape> {
    let normal = Color32::from_rgba_unmultiplied(accent[0], accent[1], accent[2], 50);
    let strong = Color32::from_rgba_unmultiplied(accent[0], accent[1], accent[2], 140);
    let mut out = Vec::new();
    for m in matches.iter().take(5000) {
        let (a, b) = (pos(m.start), pos(m.end));
        if a.bottom() < clip.top() {
            continue;
        }
        if a.top() > clip.bottom() {
            break;
        }
        // 줄이 바뀌면 첫 줄 끝까지만
        let end_x = if (a.top() - b.top()).abs() < 1.0 { b.max.x } else { right };
        let rect = egui::Rect::from_min_max(a.min, egui::pos2(end_x, a.max.y));
        out.push(Shape::rect_filled(rect, 2.0, if current == Some(m) { strong } else { normal }));
    }
    out
}

/// 왼쪽 줄 번호 (줄바꿈으로 접힌 줄에는 번호를 붙이지 않음)
#[allow(clippy::too_many_arguments)]
fn paint_line_numbers(
    painter: &egui::Painter,
    galley: &egui::Galley,
    origin: egui::Vec2,
    clip: egui::Rect,
    x_right: f32,
    row_h: f32,
    font: &FontId,
    color: Color32,
) {
    let mut line = 1;
    let mut starts_line = true;
    for row in &galley.rows {
        let rect = row.rect().translate(origin);
        if starts_line && rect.bottom() >= clip.top() {
            if rect.top() > clip.bottom() {
                break;
            }
            // 그림 줄처럼 아주 높은 줄은 위쪽에 맞춤
            let y = rect.top() + rect.height().min(row_h * 1.6) / 2.0;
            painter.text(egui::pos2(x_right, y), Align2::RIGHT_CENTER, line.to_string(), font.clone(), color);
        }
        starts_line = row.ends_with_newline;
        if starts_line {
            line += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects(spans: &[(usize, usize)]) -> Vec<(usize, usize, egui::Rect)> {
        spans.iter().map(|&(s, e)| (s, e, egui::Rect::NOTHING)).collect()
    }

    #[test]
    fn cursor_never_rests_inside_image_path() {
        let imgs = rects(&[(10, 40)]);
        let one = |p| CCursorRange::one(CCursor::new(p));
        // 위/아래로 들어오면 그림 앞
        assert_eq!(snap_out_of_images(one(25), &imgs, Some(3), true).map(|r| r.primary.index.0), Some(10));
        assert_eq!(snap_out_of_images(one(40), &imgs, Some(55), true).map(|r| r.primary.index.0), Some(10));
        // 그림 앞에서 → 는 그림 뒤, 그림 뒤에서 ← 는 그림 앞
        assert_eq!(snap_out_of_images(one(11), &imgs, Some(10), false).map(|r| r.primary.index.0), Some(40));
        assert_eq!(snap_out_of_images(one(39), &imgs, Some(40), false).map(|r| r.primary.index.0), Some(10));
        // 그림 밖이거나 이미 앞/뒤면 그대로
        assert!(snap_out_of_images(one(5), &imgs, Some(4), false).is_none());
        assert!(snap_out_of_images(one(10), &imgs, Some(3), true).is_none());
        assert!(snap_out_of_images(one(40), &imgs, Some(39), false).is_none());
    }

    #[test]
    fn cursor_info_counts_lines_and_columns() {
        let text = "첫 줄\n둘째 줄입니다\n";
        let range = CCursorRange::two(CCursor::new(4), CCursor::new(8));
        let info = cursor_info_at(text, range);
        assert_eq!((info.line, info.column, info.selected), (2, 5, 4));
    }
}
