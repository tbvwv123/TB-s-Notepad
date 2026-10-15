//! 찾기 / 바꾸기 (편집기 오른쪽 위에 떠 있는 카드).

use super::*;

impl NotepadApp {
    pub(super) fn open_find(&mut self, ctx: &egui::Context, replace: bool) {
        self.find.open = true;
        self.find.replace = replace;
        self.find.focus_query = true;
        self.prefill_find_query(ctx);
    }

    pub(super) fn prefill_find_query(&mut self, ctx: &egui::Context) {
        let id = editor_id(self.doc().id);
        if let Some(range) = TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()) {
            let r = range.as_sorted_char_range();
            let (s, e) = (r.start.0, r.end.0);
            if s != e {
                let text = &self.doc().text;
                let sel = &text[markdown::char_to_byte(text, s)..markdown::char_to_byte(text, e)];
                if !sel.contains('\n') {
                    self.find.query = sel.to_owned();
                }
            }
        }
    }

    pub(super) fn refresh_matches(&mut self) {
        let doc = &self.docs[self.active];
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            doc.id.hash(&mut h);
            doc.text.hash(&mut h);
            self.find.query.hash(&mut h);
            self.find.match_case.hash(&mut h);
            h.finish()
        };
        if key == self.find.matches_key {
            return;
        }
        self.find.matches_key = key;
        let byte_ranges = markdown::find_all(&doc.text, &self.find.query, self.find.match_case);
        // 바이트 → 글자 위치 (한 번만 훑기)
        let mut out = Vec::with_capacity(byte_ranges.len());
        let mut chars = 0;
        let mut last_byte = 0;
        for r in byte_ranges {
            chars += doc.text[last_byte..r.start].chars().count();
            let len = doc.text[r.clone()].chars().count();
            out.push(chars..chars + len);
            chars += len;
            last_byte = r.end;
        }
        self.find.matches = out;
    }

    pub(super) fn current_selection(&self, ctx: &egui::Context) -> Range<usize> {
        let id = editor_id(self.doc().id);
        TextEditState::load(ctx, id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| {
                let r = r.as_sorted_char_range();
                r.start.0..r.end.0
            })
            .unwrap_or(0..0)
    }

    pub(super) fn find_step(&mut self, ctx: &egui::Context, forward: bool) {
        if self.find.query.is_empty() {
            self.find.open = true;
            self.find.focus_query = true;
            return;
        }
        self.refresh_matches();
        if self.find.matches.is_empty() {
            self.set_status(ctx, tf!("'{}'을(를) 찾을 수 없습니다", self.find.query));
            return;
        }
        let base = self.find.current.clone().unwrap_or_else(|| self.current_selection(ctx));
        let m = &self.find.matches;
        let next = if forward {
            m.iter().find(|r| r.start >= base.end && **r != base).or(m.first())
        } else {
            m.iter().rev().find(|r| r.end <= base.start && **r != base).or(m.last())
        };
        self.find.current = next.cloned();
        // 커서를 옮겨 화면을 그 위치로 스크롤
        self.pending_select = next.cloned();
    }

    pub(super) fn close_find(&mut self) {
        self.find.open = false;
        // 찾은 부분을 선택한 채로 편집기로 돌아갑니다.
        if let Some(r) = self.find.current.take() {
            self.pending_select = Some(r);
        }
        self.focus_editor = true;
    }

    pub(super) fn replace_one(&mut self, ctx: &egui::Context) {
        self.refresh_matches();
        let current = self.find.current.clone().filter(|r| self.find.matches.contains(r));
        let Some(r) = current else {
            self.find_step(ctx, true);
            return;
        };
        let replacement = self.find.replacement.clone();
        let doc = self.doc_mut();
        let range = markdown::char_to_byte(&doc.text, r.start)..markdown::char_to_byte(&doc.text, r.end);
        doc.text.replace_range(range, &replacement);
        doc.mark_changed();
        let after = r.start + replacement.chars().count();
        self.find.current = Some(after..after);
        self.refresh_matches();
        if self.find.matches.is_empty() {
            self.find.current = None;
            self.pending_select = Some(after..after);
        } else {
            self.find_step(ctx, true);
        }
    }

    pub(super) fn replace_all(&mut self, ctx: &egui::Context) {
        let ranges = markdown::find_all(&self.doc().text, &self.find.query, self.find.match_case);
        if ranges.is_empty() {
            self.set_status(ctx, t!("바꿀 내용이 없습니다"));
            return;
        }
        let replacement = self.find.replacement.clone();
        let doc = self.doc_mut();
        let mut out = String::with_capacity(doc.text.len());
        let mut last = 0;
        for r in &ranges {
            out.push_str(&doc.text[last..r.start]);
            out.push_str(&replacement);
            last = r.end;
        }
        out.push_str(&doc.text[last..]);
        doc.text = out;
        doc.mark_changed();
        self.set_status(ctx, tf!("{}개를 바꿨습니다", ranges.len()));
    }

    /// 편집기 오른쪽 위에 떠 있는 찾기/바꾸기 카드
    pub(super) fn find_overlay(&mut self, ctx: &egui::Context, area: egui::Rect) {
        self.refresh_matches();
        let current = self.find.current.as_ref().and_then(|c| self.find.matches.iter().position(|m| m == c));
        let width = 420.0_f32.min(area.width() - 24.0).max(280.0);
        let pos = egui::pos2(area.right() - width - 18.0, area.top() + 10.0);
        let mut go: Option<bool> = None;
        let mut do_replace = false;
        let mut do_replace_all = false;

        egui::Area::new(Id::new("find_overlay")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            Frame::popup(ui.style()).inner_margin(Margin::same(8)).corner_radius(10).show(ui, |ui| {
                ui.set_width(width - 16.0);
                let base = ui.visuals().window_fill;
                let field_w = (width - 16.0 - 5.0 * 32.0 - 56.0).max(110.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.find.query)
                            .desired_width(field_w)
                            .hint_text(t!("찾기"))
                            .margin(Margin::symmetric(8, 5)),
                    );
                    if self.find.focus_query {
                        resp.request_focus();
                        self.find.focus_query = false;
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        go = Some(!ui.input(|i| i.modifiers.shift));
                        resp.request_focus();
                    }
                    let count = self.find.matches.len();
                    let info = match (count, current) {
                        (0, _) if !self.find.query.is_empty() => t!("없음").to_owned(),
                        (0, _) => String::new(),
                        (n, Some(i)) => format!("{}/{n}", i + 1),
                        (n, None) => tf!("{}개", n),
                    };
                    ui.add_sized([52.0, 26.0], egui::Label::new(RichText::new(info).small().weak()));
                    if ui_kit::text_button(
                        ui,
                        RichText::new("Aa").size(13.0),
                        t!("대소문자 구분"),
                        self.find.match_case,
                        base,
                    )
                    .clicked()
                    {
                        self.find.match_case = !self.find.match_case;
                    }
                    if ui_kit::icon_button(ui, Icon::ChevronUp, t!("이전 (Shift+Enter, Shift+F3)"), false, base)
                        .clicked()
                    {
                        go = Some(false);
                    }
                    if ui_kit::icon_button(ui, Icon::ChevronDown, t!("다음 (Enter, F3)"), false, base).clicked() {
                        go = Some(true);
                    }
                    if ui_kit::icon_button(ui, Icon::Replace, t!("바꾸기 (Ctrl+H)"), self.find.replace, base).clicked()
                    {
                        self.find.replace = !self.find.replace;
                    }
                    if ui_kit::icon_button(ui, Icon::Close, t!("닫기 (Esc)"), false, base).clicked() {
                        self.close_find();
                    }
                });
                if self.find.replace {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let resp = ui.add(
                            TextEdit::singleline(&mut self.find.replacement)
                                .desired_width(field_w)
                                .hint_text(t!("바꿀 내용"))
                                .margin(Margin::symmetric(8, 5)),
                        );
                        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            do_replace = true;
                            resp.request_focus();
                        }
                        ui.add_space(54.0);
                        if ui.button(t!("바꾸기")).clicked() {
                            do_replace = true;
                        }
                        if ui.button(t!("모두 바꾸기")).clicked() {
                            do_replace_all = true;
                        }
                    });
                }
            });
        });
        if let Some(forward) = go {
            self.find_step(ctx, forward);
        }
        if do_replace {
            self.replace_one(ctx);
        }
        if do_replace_all {
            self.replace_all(ctx);
        }
    }
}
