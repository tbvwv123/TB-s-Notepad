//! 모달 대화상자 (저장 확인, 알림, 휴지통, 새 폴더, 새 이모지, 밖에서 바뀐 파일).

use super::*;

impl NotepadApp {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else { return };
        enum Choice {
            Primary,
            Discard,
            Cancel,
        }
        let mut choice = None;
        egui::Modal::new(Id::new("dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            let (title, body, primary, discard): (&str, String, &str, Option<&str>) = match dialog {
                Dialog::CloseDoc(_) => (
                    t!("저장할까요?"),
                    t!("이 문서의 변경 내용이 아직 파일로 저장되지 않았습니다.").into(),
                    t!("저장"),
                    Some(t!("저장 안 함")),
                ),
                Dialog::Exit => (
                    t!("저장할까요?"),
                    t!("저장하지 않은 문서가 있습니다. 끝내기 전에 저장할까요?").into(),
                    t!("모두 저장"),
                    Some(t!("저장 안 함")),
                ),
                Dialog::Message(m) => (t!("알림"), m.clone(), t!("확인"), None),
                Dialog::ConfirmDelete(p) => (
                    t!("휴지통으로 옮길까요?"),
                    tf!(
                        "{}\n\n휴지통에서 다시 꺼낼 수 있습니다.",
                        p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    ),
                    t!("휴지통으로"),
                    None,
                ),
                Dialog::NewFolder { .. } => (t!("새 폴더"), String::new(), t!("만들기"), None),
                Dialog::RegisterEmoji { .. } => {
                    (t!("새 이모지"), t!("이름을 정하면 글에서 :이름: 으로 넣을 수 있습니다.").into(), t!("추가"), None)
                }
                Dialog::ExternalChange(id) => {
                    let name = self.docs.iter().find(|d| d.id == *id).map(doc_display_title).unwrap_or_default();
                    (
                        t!("파일이 밖에서 바뀌었습니다"),
                        tf!(
                            "{}\n\n다른 프로그램(AI 도구, 동기화, 다른 편집기 등)이 이 파일을 고쳤는데, \
                             앱에서 고친 내용도 아직 저장되지 않았습니다. 고를 때까지 자동 저장을 멈춥니다.\n\n\
                             · 다시 불러오기: 파일의 새 내용을 엽니다 (앱에서 고친 내용은 사라집니다)\n\
                             · 내 내용 유지: 앱의 내용으로 파일을 덮어씁니다",
                            name
                        ),
                        t!("다시 불러오기"),
                        Some(t!("내 내용 유지")),
                    )
                }
            };
            ui.label(RichText::new(title).size(17.0).strong());
            ui.add_space(8.0);
            if let Dialog::NewFolder { name, .. } | Dialog::RegisterEmoji { name, .. } = dialog {
                if !body.is_empty() {
                    ui.label(body);
                    ui.add_space(6.0);
                }
                let resp =
                    ui.add(TextEdit::singleline(name).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 6)));
                if !resp.has_focus() && !resp.lost_focus() {
                    resp.request_focus();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    choice = Some(Choice::Primary);
                }
            } else {
                ui.label(body);
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let primary_btn = egui::Button::new(RichText::new(primary).color(Color32::WHITE))
                    .fill(c(self.settings.palette().accent));
                if ui.add(primary_btn).clicked() {
                    choice = Some(Choice::Primary);
                }
                if let Some(d) = discard
                    && ui.button(d).clicked()
                {
                    choice = Some(Choice::Discard);
                }
                if !matches!(dialog, Dialog::Message(_) | Dialog::ExternalChange(_)) && ui.button(t!("취소")).clicked()
                {
                    choice = Some(Choice::Cancel);
                }
            });
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                choice = Some(Choice::Cancel);
            }
        });

        let Some(choice) = choice else { return };
        let dialog = self.dialog.take();
        match (dialog, choice) {
            (Some(Dialog::CloseDoc(id)), Choice::Primary) => {
                if let Some(i) = self.docs.iter().position(|d| d.id == id)
                    && self.save_doc(ctx, i, false)
                {
                    self.close_doc(id);
                }
            }
            (Some(Dialog::CloseDoc(id)), Choice::Discard) => self.close_doc(id),
            (Some(Dialog::Exit), Choice::Primary) => {
                if self.save_all(ctx) {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
            (Some(Dialog::Exit), Choice::Discard) => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            (Some(Dialog::ConfirmDelete(p)), Choice::Primary) => self.delete_path(ctx, &p),
            (Some(Dialog::ExternalChange(id)), Choice::Primary) => {
                if let Some(i) = self.docs.iter().position(|d| d.id == id) {
                    let old_text = self.docs[i].text.clone();
                    match self.docs[i].reload() {
                        Ok(()) => self.after_reload(ctx, i, &old_text),
                        Err(err) => self.dialog = Some(Dialog::Message(tf!("다시 불러오지 못했습니다.\n\n{}", err))),
                    }
                }
            }
            (Some(Dialog::ExternalChange(id)), Choice::Discard) => {
                if let Some(d) = self.docs.iter_mut().find(|d| d.id == id) {
                    d.keep_mine();
                }
            }
            // 꼭 골라야 하는 대화상자 (Esc로 닫지 않음)
            (Some(d @ Dialog::ExternalChange(_)), Choice::Cancel) => self.dialog = Some(d),
            (Some(Dialog::RegisterEmoji { source, dir, name, insert_at }), Choice::Primary) => {
                self.finish_register_emoji(ctx, &source, &dir, &name, insert_at);
            }
            (Some(Dialog::NewFolder { parent, name }), Choice::Primary) => {
                let clean = sanitize_file_name(&name);
                if !clean.is_empty() {
                    let dir = parent.join(clean);
                    if let Err(err) = std::fs::create_dir(&dir) {
                        self.dialog = Some(Dialog::Message(tf!("폴더를 만들지 못했습니다.\n{}", err)));
                    }
                    self.sidebar.invalidate();
                }
            }
            _ => {}
        }
        self.focus_editor = self.dialog.is_none();
    }
}
