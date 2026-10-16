//! 사이드바 그리기 부품: 줄 · 머리 · 아이콘 단추 · 검색 칸 · 정렬 창 · 끌어 놓기 표시.

use super::*;

pub(super) enum RowIcon {
    Folder { open: bool },
    Note,
}

pub(super) const ROW_H: f32 = 30.0;

/// 끌고 온 파일·폴더를 이 줄에 놓으면 `dir` 로 옮기기. 끌고 있는 동안 놓을 자리를 강조합니다.
pub(super) fn drop_target(ui: &Ui, resp: &Response, dir: &Path, actions: &mut Vec<Action>, k: Colors) {
    if let Some(from) = resp.dnd_hover_payload::<PathBuf>()
        && !dir.starts_with(from.as_path())
    {
        ui.painter().rect_stroke(resp.rect, 8.0, egui::Stroke::new(1.5, k.accent), egui::StrokeKind::Inside);
    }
    if let Some(from) = resp.dnd_release_payload::<PathBuf>() {
        actions.push(Action::Move { from: (*from).clone(), to_dir: dir.to_path_buf() });
    }
}

/// 끌고 있는 파일·폴더 이름을 마우스 옆에 보여 줍니다.
pub(super) fn drag_ghost(ui: &Ui, k: Colors) {
    let Some(path) = egui::DragAndDrop::payload::<PathBuf>(ui.ctx()) else { return };
    let Some(pos) = ui.ctx().pointer_interact_pos() else { return };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("sidebar_drag")));
    let galley = painter.layout_no_wrap(name, FontId::proportional(13.0), k.text);
    let rect = Rect::from_min_size(pos + vec2(14.0, 10.0), galley.size() + vec2(16.0, 8.0));
    painter.rect_filled(rect, 6.0, k.panel);
    painter.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, k.accent), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(8.0, 4.0), galley, k.text);
}

/// 줄 오른쪽 · 글자에 붙는 표시
#[derive(Clone, Copy, Default)]
pub(super) struct RowMark {
    /// 저장 안 됨 (점)
    pub(super) dirty: bool,
    /// 상단 고정 (핀)
    pub(super) pinned: bool,
    /// 숨긴 것 (흐리게)
    pub(super) dim: bool,
}

pub(super) fn tree_row(
    ui: &mut Ui,
    depth: usize,
    icon: RowIcon,
    label: &str,
    active: bool,
    mark: RowMark,
    k: Colors,
) -> Response {
    // 누르기와 끌기(다른 폴더로 옮기기)를 함께 받음
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    let hovered = resp.hovered() || ui.rect_contains_pointer(rect);
    let fill = if active {
        mix(k.panel, k.text, 0.09)
    } else if hovered {
        mix(k.panel, k.text, 0.05)
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, 8.0, fill);
    let x = rect.left() + 10.0 + depth as f32 * 14.0;
    let cy = rect.center().y;
    let icon_color = if active { k.text } else { k.muted };
    match icon {
        RowIcon::Folder { open } => {
            let chev = if open { Icon::ChevronDown } else { Icon::ChevronRight };
            ui_kit::paint_icon(
                p,
                Rect::from_center_size(pos2(x + 6.0, cy), vec2(11.0, 11.0)),
                chev,
                icon_color,
                k.panel,
            );
        }
        RowIcon::Note => {
            p.circle_stroke(pos2(x + 6.0, cy), 4.5, egui::Stroke::new(1.3, icon_color));
        }
    }
    let text_color = if active { k.text } else { mix(k.text, k.panel, 0.12) };
    let text_color = if mark.dim { mix(text_color, k.panel, 0.55) } else { text_color };
    let max_w = rect.right() - (x + 20.0) - 34.0;
    let galley = elided(ui, label, 13.5, max_w, text_color);
    ui.painter().galley(pos2(x + 20.0, cy - galley.size().y / 2.0), galley, text_color);
    if mark.dirty {
        ui.painter().circle_filled(pos2(rect.right() - 14.0, cy), 3.0, k.accent);
    } else if mark.pinned {
        // 핀: 머리 · 바늘
        let c = pos2(rect.right() - 14.0, cy - 2.0);
        ui.painter().circle_filled(c, 3.2, k.muted);
        ui.painter().line_segment([c, c + vec2(0.0, 7.0)], egui::Stroke::new(1.4, k.muted));
    }
    resp
}

pub(super) fn elided(ui: &Ui, text: &str, size: f32, max_w: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), FontId::proportional(size), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(20.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

pub(super) fn root_header(ui: &mut Ui, name: &str, collapsed: bool, k: Colors) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    let p = ui.painter();
    let cy = rect.center().y;
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        p.rect_filled(rect, 8.0, mix(k.panel, k.text, 0.04));
    }
    ui_kit::paint_icon(
        p,
        Rect::from_center_size(pos2(rect.left() + 16.0, cy), vec2(15.0, 15.0)),
        Icon::Folder,
        k.muted,
        k.panel,
    );
    let galley = elided(ui, name, 12.5, rect.width() - 80.0, k.muted);
    ui.painter().galley(pos2(rect.left() + 30.0, cy - galley.size().y / 2.0), galley, k.muted);
    if hovered {
        let chev = if collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
        ui_kit::paint_icon(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 44.0, cy), vec2(10.0, 10.0)),
            chev,
            k.muted,
            k.panel,
        );
    }
    resp
}

/// 줄 오른쪽 끝에 마우스를 올렸을 때만 보이는 작은 아이콘 버튼. 눌렀으면 `true`
pub(super) fn hover_icon(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors) -> bool {
    if !ui.rect_contains_pointer(row.rect) {
        return false;
    }
    small_icon_at(ui, row, icon, tip, k)
}

/// 정렬 창: 폴더 정렬, 메모 정렬 드롭다운
pub(super) fn sort_menu(ui: &mut Ui, sort: &mut SidebarSort) {
    for (n, (title, order)) in
        [(t!("폴더 정렬"), &mut sort.folders), (t!("메모 정렬"), &mut sort.notes)].into_iter().enumerate()
    {
        ui.label(RichText::new(title).small().weak());
        egui::ComboBox::from_id_salt(("sidebar_sort", n)).selected_text(order.label()).width(230.0).show_ui(ui, |ui| {
            for o in SortOrder::ALL {
                ui.selectable_value(order, o, o.label());
            }
        });
        ui.add_space(6.0);
    }
}

/// 줄 오른쪽 끝에서 `dx` 만큼 안쪽에 늘 보이는 작은 아이콘 단추
pub(super) fn header_icon(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors, dx: f32) -> Response {
    let r = Rect::from_center_size(pos2(row.rect.right() - dx, row.rect.center().y), vec2(22.0, 22.0));
    let resp = ui.interact(r, row.id.with(("header_icon", icon as u8)), Sense::click()).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, 6.0, mix(k.panel, k.text, 0.10));
    }
    let color = if resp.hovered() { k.text } else { k.muted };
    ui_kit::paint_icon(ui.painter(), r.shrink(5.0), icon, color, k.panel);
    resp
}

pub(super) fn hover_icon_always(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors) -> bool {
    small_icon_at(ui, row, icon, tip, k)
}

pub(super) fn small_icon_at(ui: &mut Ui, row: &Response, icon: Icon, tip: &str, k: Colors) -> bool {
    let r = Rect::from_center_size(pos2(row.rect.right() - 16.0, row.rect.center().y), vec2(22.0, 22.0));
    let resp = ui.interact(r, row.id.with(("hover_icon", icon as u8)), Sense::click()).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, 6.0, mix(k.panel, k.text, 0.10));
    }
    let color = if resp.hovered() { k.text } else { k.muted };
    ui_kit::paint_icon(ui.painter(), r.shrink(5.0), icon, color, k.panel);
    resp.clicked()
}

pub(super) fn section_label(ui: &mut Ui, text: &str, k: Colors) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.0),
        k.muted,
    );
}

/// 여러 줄로 접히는 흐린 도움말
pub(super) fn hint_label(ui: &mut Ui, text: &str, k: Colors) {
    ui.add_space(4.0);
    egui::Frame::NONE.inner_margin(egui::Margin { left: 10, right: 8, top: 0, bottom: 4 }).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(k.muted)).wrap());
    });
}

pub(super) fn action_row(ui: &mut Ui, icon: Icon, label: &str, shortcut: &str, k: Colors) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 8.0, mix(k.panel, k.text, 0.06));
    }
    let cy = rect.center().y;
    let c = pos2(rect.left() + 18.0, cy);
    p.circle_filled(c, 10.0, mix(k.panel, k.text, 0.08));
    ui_kit::paint_icon(p, Rect::from_center_size(c, vec2(11.0, 11.0)), icon, k.text, k.panel);
    p.text(pos2(rect.left() + 36.0, cy), Align2::LEFT_CENTER, label, FontId::proportional(14.0), k.text);
    if !shortcut.is_empty() && resp.hovered() {
        p.text(pos2(rect.right() - 10.0, cy), Align2::RIGHT_CENTER, shortcut, FontId::proportional(11.5), k.muted);
    }
    resp
}

pub(super) fn search_box(ui: &mut Ui, query: &mut String, k: Colors) {
    let frame = egui::Frame::NONE
        .fill(mix(k.panel, k.text, 0.05))
        .stroke(egui::Stroke::new(1.0, mix(k.panel, k.text, 0.10)))
        .corner_radius(9)
        .inner_margin(egui::Margin { left: 30, right: 8, top: 5, bottom: 5 });
    let resp = frame.show(ui, |ui| {
        ui.add(
            TextEdit::singleline(query)
                .id(egui::Id::new("sidebar_search"))
                .hint_text(t!("검색  (Ctrl+P)"))
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY),
        )
    });
    let r = resp.response.rect;
    ui_kit::paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(13.0, 13.0)),
        Icon::Search,
        k.muted,
        k.panel,
    );
    if !query.is_empty() {
        let x = Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(16.0, 16.0));
        let xr = ui.interact(x, resp.response.id.with("clear"), Sense::click());
        ui_kit::paint_icon(
            ui.painter(),
            x.shrink(3.0),
            Icon::Close,
            if xr.hovered() { k.text } else { k.muted },
            k.panel,
        );
        if xr.clicked() {
            query.clear();
        }
    }
}
