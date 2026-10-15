//! 설정 창 (테마와 색, 글꼴, 편집기, 단축키, 정보).

use super::*;

impl NotepadApp {
    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new(t!("설정"))
            .id(Id::new("settings_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size([640.0, 470.0])
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    // 왼쪽 메뉴
                    ui.vertical(|ui| {
                        ui.set_width(132.0);
                        for page in SettingsPage::ALL {
                            if nav_item(ui, page.label(), self.settings_page == page).clicked() {
                                self.settings_page = page;
                            }
                        }
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ScrollArea::vertical().id_salt("settings_scroll").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_width(ui.available_width() - 10.0);
                            match self.settings_page {
                                SettingsPage::Theme => self.settings_theme(ui),
                                SettingsPage::Font => self.settings_font(ui),
                                SettingsPage::Editor => self.settings_editor(ui),
                                SettingsPage::Keys => settings_keys(ui),
                                SettingsPage::About => settings_about(ui),
                            }
                        });
                    });
                });
            });
        self.show_settings = open;
    }

    pub(super) fn settings_theme(&mut self, ui: &mut Ui) {
        ui_kit::section_title(ui, t!("테마"));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            for preset in ThemePreset::ALL {
                let palette = Palette::preset(preset, &self.settings.custom_palette);
                if theme_card(ui, preset.label(), &palette, self.settings.theme == preset).clicked() {
                    self.settings.theme = preset;
                }
            }
        });
        ui.add_space(14.0);
        ui_kit::section_title(ui, t!("사용자 지정 색"));
        if self.settings.theme != ThemePreset::Custom {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!("색을 바꾸면 '사용자 지정' 테마로 바뀝니다.")).small().weak());
                if ui.small_button(t!("지금 테마에서 시작")).clicked() {
                    self.settings.custom_palette = self.settings.palette();
                    self.settings.theme = ThemePreset::Custom;
                }
            });
            ui.add_space(4.0);
        }
        let mut p = self.settings.custom_palette;
        let before = p;
        ui_kit::switch_row(ui, t!("어두운 바탕"), t!("글자 대비와 그림자를 어두운 화면에 맞춥니다"), &mut p.dark);
        egui::Grid::new("colors").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
            let mut fields: Vec<(&str, &mut [u8; 3])> = vec![
                (t!("본문 배경"), &mut p.editor_bg),
                (t!("사이드바 배경"), &mut p.panel_bg),
                (t!("글자"), &mut p.text),
                (t!("흐린 글자"), &mut p.muted),
                (t!("강조 · 선택"), &mut p.accent),
                (t!("제목"), &mut p.heading),
                (t!("코드"), &mut p.code),
                (t!("링크"), &mut p.link),
                (t!("인용"), &mut p.quote),
            ];
            for (i, (label, col)) in fields.iter_mut().enumerate() {
                ui.color_edit_button_srgb(col);
                ui.label(*label);
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        if p != before {
            self.settings.custom_palette = p;
            self.settings.theme = ThemePreset::Custom;
        }
    }

    pub(super) fn settings_font(&mut self, ui: &mut Ui) {
        let palette = self.settings.palette();
        ui_kit::section_title(ui, t!("본문"));
        egui::Grid::new("fonts1").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(t!("글꼴"));
            let default_doc = crate::fonts::default_font_label(crate::i18n::lang());
            font_picker(ui, "doc_font", &mut self.settings.doc_font, &self.fonts, &mut self.font_filter, default_doc);
            ui.end_row();
            ui.label(t!("크기"));
            ui.add(
                egui::Slider::new(&mut self.settings.editor_font_size, 8.0..=48.0)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" pt"),
            );
            ui.end_row();
        });
        ui.add_space(8.0);
        Frame::NONE
            .fill(c(palette.editor_bg))
            .stroke(Stroke::new(1.0, mix(c(palette.editor_bg), c(palette.text), 0.1)))
            .inner_margin(Margin::symmetric(14, 12))
            .corner_radius(8)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let size = self.settings.editor_font_size;
                // 제목 모양 (굵은 글꼴이 없으면 본문 편집기처럼 겹쳐 그려 굵게)
                let heading = c(palette.heading);
                let galley = ui.painter().layout_no_wrap(
                    t!("다람쥐 헌 쳇바퀴에 타고파").to_owned(),
                    FontId::new(size * 1.3, bold_family()),
                    heading,
                );
                let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                if self.fonts.fake_bold {
                    for dx in crate::fonts::fake_bold_offsets(size * 1.3, ui.ctx().pixels_per_point()) {
                        ui.painter().galley(rect.min + egui::vec2(dx, 0.0), galley.clone(), heading);
                    }
                }
                ui.painter().galley(rect.min, galley, heading);
                ui.label(
                    RichText::new("The quick brown fox jumps over the lazy dog. 0123")
                        .font(FontId::new(size, doc_family()))
                        .color(c(palette.text)),
                );
            });
        // 고른 글꼴로 다 못 보여 주는 경우 알려 줌
        if let Some(name) = self.settings.doc_font.clone() {
            let mut notes = Vec::new();
            // 화면 언어의 글자 (한글 · 간체 한자 · 가나)가 없으면 그 글자는 기본 글꼴로 대신 보임
            let sample = match crate::i18n::lang() {
                Lang::Ko => Some('가'),
                Lang::Zh => Some('汉'),
                Lang::Ja => Some('あ'),
                Lang::En => None,
            };
            if sample.is_some_and(|ch| self.fonts.has_char(&name, ch) == Some(false)) {
                notes.push(t!("이 글꼴에는 이 언어의 글자가 없어서 그 글자는 기본 글꼴로 보입니다."));
            }
            if self.fonts.fake_bold {
                notes.push(t!("이 글꼴에는 굵은 글꼴이 없어서 제목과 굵은 글씨는 글자를 겹쳐 굵게 보이게 합니다."));
            }
            for n in notes {
                ui.add_space(4.0);
                ui.label(RichText::new(n).small().color(c(palette.muted)));
            }
        }
        ui.add_space(14.0);
        ui_kit::section_title(ui, t!("원본 보기 (코드)"));
        egui::Grid::new("fonts2").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(t!("글꼴"));
            font_picker(
                ui,
                "editor_font",
                &mut self.settings.editor_font,
                &self.fonts,
                &mut self.font_filter,
                t!("기본 (고정폭)"),
            );
            ui.end_row();
            ui.label(t!("줄 간격"));
            ui.add(
                egui::Slider::new(&mut self.settings.line_spacing, 1.0..=2.5)
                    .step_by(0.05)
                    .fixed_decimals(2)
                    .suffix(t!(" 배")),
            );
            ui.end_row();
        });
        ui.add_space(14.0);
        ui_kit::section_title(ui, t!("화면"));
        egui::Grid::new("fonts3").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(t!("글꼴"));
            font_picker(
                ui,
                "ui_font",
                &mut self.settings.ui_font,
                &self.fonts,
                &mut self.font_filter,
                &tf!("기본 ({})", crate::fonts::default_font_label(crate::i18n::lang())),
            );
            ui.end_row();
            ui.label(t!("배율"));
            let mut scale = self.settings.ui_scale;
            let resp =
                ui.add(egui::Slider::new(&mut scale, 0.75..=2.0).step_by(0.05).fixed_decimals(2).suffix(t!(" 배")));
            // 끄는 동안 배율을 바꾸면 슬라이더가 손 밑에서 움직이므로 놓았을 때 적용
            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                self.settings.ui_scale = scale;
            }
            ui.end_row();
        });
    }

    pub(super) fn settings_editor(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        // 어느 언어로 보고 있어도 찾을 수 있게 "Language" 를 함께 씀
        ui_kit::section_title(ui, &tf!("{} · Language", t!("언어")));
        let auto = tf!("자동 ({})", Lang::system().native_name());
        let current = s.language.map_or(auto.clone(), |l| l.native_name().to_owned());
        egui::ComboBox::from_id_salt("language").selected_text(current).width(220.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.language, None, auto);
            for l in Lang::ALL {
                ui.selectable_value(&mut s.language, Some(l), l.native_name());
            }
        });
        ui.add_space(12.0);
        ui_kit::section_title(ui, t!("저장"));
        ui_kit::switch_row(ui, t!("자동 저장"), t!("입력을 멈추면 바로 파일에 저장합니다"), &mut s.autosave);
        ui_kit::switch_row(
            ui,
            t!("열린 문서 기억"),
            t!("저장하지 않은 새 문서까지 다음 실행 때 복원합니다"),
            &mut s.restore_session,
        );
        ui_kit::switch_row(
            ui,
            t!("그림을 문서 폴더에 함께 보관"),
            t!("다른 곳의 그림을 넣거나 저장할 때 문서 옆 그림 폴더로 복사합니다 (백업·이동용)"),
            &mut s.keep_images_with_doc,
        );
        ui.horizontal(|ui| {
            ui.label(t!("붙여넣은 그림 폴더"));
            ui.add(TextEdit::singleline(&mut s.image_folder).desired_width(110.0));
            ui.label(RichText::new(t!("문서 기준")).small().weak());
        });
        ui.add_space(12.0);
        ui_kit::section_title(ui, t!("보기"));
        let mut source = s.view_mode == ViewMode::Source;
        if ui_kit::switch_row(
            ui,
            t!("마크다운 원본 보기"),
            t!("기호(#, ** 등)를 그대로 보면서 편집합니다 (Ctrl+E)"),
            &mut source,
        ) {
            s.view_mode = if source { ViewMode::Source } else { ViewMode::Document };
        }
        let mut limit = s.max_text_width > 0.0;
        if ui_kit::switch_row(ui, t!("본문 폭 제한"), t!("넓은 창에서도 읽기 좋은 너비로 가운데 정렬"), &mut limit)
        {
            s.max_text_width = if limit { 860.0 } else { 0.0 };
        }
        if limit {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.add(egui::Slider::new(&mut s.max_text_width, 500.0..=1400.0).step_by(20.0).suffix(" px"));
            });
            ui.add_space(6.0);
        }
        ui_kit::switch_row(ui, t!("줄 번호"), "", &mut s.show_line_numbers);
        ui_kit::switch_row(ui, t!("서식 도구"), "", &mut s.show_toolbar);
        ui_kit::switch_row(ui, t!("상태 표시줄"), "", &mut s.show_status_bar);
        ui_kit::switch_row(ui, t!("사이드바"), t!("Ctrl+\\ 로 열고 닫기"), &mut s.sidebar_visible);
        ui_kit::switch_row(ui, t!("항상 위에 표시"), "", &mut s.always_on_top);
        ui.add_space(8.0);
        ui_kit::section_title(ui, t!("원본 보기"));
        ui_kit::switch_row(ui, t!("자동 줄 바꿈"), "Alt+Z", &mut s.word_wrap);
        ui_kit::switch_row(ui, t!("현재 줄 강조"), "", &mut s.highlight_current_line);
        ui_kit::switch_row(ui, t!("마크다운 문법 강조"), "", &mut s.markdown_highlight);
        ui.add_space(16.0);
        if ui.button(t!("모든 설정 기본값으로")).clicked() {
            let keep = (std::mem::take(&mut s.recent_files), std::mem::take(&mut s.roots));
            *s = Settings { recent_files: keep.0, roots: keep.1, roots_initialized: true, ..Settings::default() };
        }
    }
}

pub(super) fn font_picker(
    ui: &mut Ui,
    id: &str,
    current: &mut Option<String>,
    fonts: &FontLibrary,
    filter: &mut String,
    default_label: &str,
) {
    let selected = current.clone().unwrap_or_else(|| default_label.to_owned());
    egui::ComboBox::from_id_salt(id).selected_text(selected).width(240.0).height(360.0).show_ui(ui, |ui| {
        ui.add(TextEdit::singleline(filter).hint_text(t!("글꼴 검색")).desired_width(f32::INFINITY));
        ui.separator();
        ui.selectable_value(current, None, default_label);
        let f = filter.to_lowercase();
        for font in fonts.fonts.iter().filter(|x| f.is_empty() || x.name.to_lowercase().contains(&f)) {
            ui.selectable_value(current, Some(font.name.clone()), &font.name);
        }
    });
}

/// 설정 창 왼쪽 메뉴 한 줄
pub(super) fn nav_item(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let k = ui_kit::colors(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    let fill = if selected {
        mix(k.panel, k.accent, 0.14)
    } else if resp.hovered() {
        mix(k.panel, k.text, 0.06)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let color = if selected { k.accent } else { k.text };
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        color,
    );
    resp
}

/// 테마 미리보기 카드
pub(super) fn theme_card(ui: &mut Ui, name: &str, p: &Palette, selected: bool) -> egui::Response {
    let k = ui_kit::colors(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(138.0, 104.0), egui::Sense::click());
    let painter = ui.painter();
    let preview = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 76.0));
    painter.rect_filled(preview, 10.0, c(p.editor_bg));
    // 왼쪽 사이드바 띠
    let side = egui::Rect::from_min_size(preview.min, egui::vec2(34.0, preview.height()));
    painter.rect_filled(side, egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }, c(p.panel_bg));
    for i in 0..3 {
        let y = preview.top() + 14.0 + i as f32 * 10.0;
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(preview.left() + 7.0, y), egui::vec2(20.0, 4.0)),
            2.0,
            mix(c(p.panel_bg), c(p.text), 0.35),
        );
    }
    let x = preview.left() + 44.0;
    let bar = |y: f32, w: f32, h: f32, col: Color32| {
        painter.rect_filled(
            egui::Rect::from_min_size(egui::pos2(x, preview.top() + y), egui::vec2(w, h)),
            h / 2.0,
            col,
        );
    };
    bar(14.0, 56.0, 7.0, c(p.heading));
    bar(28.0, 80.0, 4.0, mix(c(p.editor_bg), c(p.text), 0.75));
    bar(37.0, 66.0, 4.0, mix(c(p.editor_bg), c(p.text), 0.75));
    bar(50.0, 28.0, 6.0, c(p.accent));
    painter.rect_filled(
        egui::Rect::from_min_size(egui::pos2(x + 34.0, preview.top() + 50.0), egui::vec2(24.0, 6.0)),
        3.0,
        c(p.code),
    );
    let border = if selected {
        Stroke::new(2.0, k.accent)
    } else if resp.hovered() {
        Stroke::new(1.0, mix(k.panel, k.text, 0.35))
    } else {
        Stroke::new(1.0, mix(k.panel, k.text, 0.12))
    };
    painter.rect_stroke(preview, 10.0, border, egui::StrokeKind::Inside);
    let label_color = if selected { k.accent } else { k.text };
    painter.text(
        egui::pos2(rect.center().x, preview.bottom() + 15.0),
        Align2::CENTER_CENTER,
        name,
        FontId::proportional(12.5),
        label_color,
    );
    resp
}

pub(super) fn settings_keys(ui: &mut Ui) {
    ui_kit::section_title(ui, t!("단축키"));
    egui::Grid::new("keys").num_columns(2).spacing([28.0, 8.0]).show(ui, |ui| {
        for (k, d) in [
            ("Ctrl+N", t!("새 메모")),
            ("Ctrl+O / S", t!("파일 열기 / 저장")),
            ("Ctrl+P", t!("메모 검색")),
            ("Ctrl+\\", t!("사이드바 열고 닫기")),
            ("F2", t!("지금 메모 이름 바꾸기")),
            ("Ctrl+W", t!("문서 닫기")),
            ("Ctrl+F / H", t!("찾기 / 바꾸기")),
            ("Ctrl+Z / Ctrl+Y", t!("실행 취소 / 다시 실행")),
            ("Ctrl+V", t!("붙여넣기 (복사한 그림도)")),
            ("Ctrl+B / I / K", t!("굵게 / 기울임 / 링크")),
            ("Ctrl+Alt+1~3", t!("제목 1~3")),
            (t!("Ctrl+클릭"), t!("링크 열기")),
            (t!("그림 클릭"), t!("크기·위치 도구, 모서리 끌어 크기 조절, Delete 로 지우기")),
            ("Ctrl+E", t!("마크다운 원본 보기")),
            ("F5", t!("날짜/시간 넣기")),
            (t!("Ctrl+휠 · Ctrl+= / -"), t!("글자 크기")),
            ("Ctrl+,", t!("설정")),
            ("F11", t!("전체 화면")),
        ] {
            ui.label(RichText::new(k).monospace().size(12.5));
            ui.label(d);
            ui.end_row();
        }
    });
    ui.add_space(10.0);
    ui.label(
        RichText::new(t!("파일을 창에 끌어다 놓으면 열리고, 그림은 문서에, 폴더는 사이드바에 들어갑니다."))
            .small()
            .weak(),
    );
}

pub(super) fn settings_about(ui: &mut Ui) {
    ui.add_space(8.0);
    ui.label(RichText::new(APP_NAME).size(22.0).strong());
    ui.label(RichText::new(tf!("버전 {}", env!("CARGO_PKG_VERSION"))).weak());
    ui.add_space(8.0);
    ui.label(t!("Rust와 egui로 만든 가볍고 쉬운 메모장"));
    ui.add_space(16.0);
    ui_kit::section_title(ui, t!("설정 저장 위치"));
    ui.horizontal(|ui| {
        ui.label(RichText::new(app_data_dir().display().to_string()).small());
        if ui.small_button(t!("열기")).clicked() {
            let _ = std::fs::create_dir_all(app_data_dir());
            let _ = open_in_file_manager(&app_data_dir());
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new(t!(
            "실행 파일 옆에 portable.txt 파일을 만들어 두면 설정을 실행 파일 옆 data 폴더에 저장합니다 (USB 휴대용)."
        ))
        .small()
        .weak(),
    );
}
