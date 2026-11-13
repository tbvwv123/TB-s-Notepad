//! 편집기 커서(선택 영역) 읽기 · 옮기기.
//! egui 는 커서를 TextEdit 의 상태에 두므로, 그 상태를 읽고 쓰는 일을 여기에 모았습니다.
//! 위치는 모두 글자 단위입니다.

use super::*;

/// 커서 범위 (선택이 없으면 빈 범위). 편집기를 아직 그린 적이 없으면 `None`
pub(super) fn cursor_range(ctx: &egui::Context, id: Id) -> Option<CCursorRange> {
    TextEditState::load(ctx, id)?.cursor.char_range()
}

/// 커서(선택했다면 움직이는 쪽 끝)의 위치
pub(super) fn cursor_index(ctx: &egui::Context, id: Id) -> Option<usize> {
    cursor_range(ctx, id).map(|r| r.primary.index.0)
}

/// 선택 없이 서 있는 커서의 위치 (선택 중이면 `None`)
pub(super) fn caret_index(ctx: &egui::Context, id: Id) -> Option<usize> {
    cursor_range(ctx, id).filter(CCursorRange::is_empty).map(|r| r.primary.index.0)
}

/// 선택 영역을 앞에서 뒤로 (선택이 없으면 커서 자리의 빈 범위)
pub(super) fn selection(ctx: &egui::Context, id: Id) -> Option<Range<usize>> {
    cursor_range(ctx, id).map(|r| {
        let r = r.as_sorted_char_range();
        r.start.0..r.end.0
    })
}

pub(super) fn set_cursor_range(ctx: &egui::Context, id: Id, range: CCursorRange) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
}

/// 선택을 풀고 커서를 `at` 에 둡니다.
pub(super) fn set_caret(ctx: &egui::Context, id: Id, at: usize) {
    set_cursor_range(ctx, id, CCursorRange::one(CCursor::new(at)));
}

/// `sel` 을 선택합니다 (커서는 끝 쪽).
pub(super) fn set_selection(ctx: &egui::Context, id: Id, sel: Range<usize>) {
    set_cursor_range(ctx, id, CCursorRange::two(CCursor::new(sel.start), CCursor::new(sel.end)));
}
