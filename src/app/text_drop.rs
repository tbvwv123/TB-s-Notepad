//! 다른 프로그램에서 끌어온 글 놓기: 넣을 자리 정하기와 넣기.
//! (창이 글을 받는 부분은 `crate::drop_target`, 놓을 자리 표시는 `editor`)

use super::*;

/// 끌어온 글을 넣을 자리 (글자 위치)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DropSpot {
    /// 이 자리에 그대로
    At(usize),
    /// 이 자리에서 시작하는 그림 줄 위에 새 줄을 만들어
    AboveImage(usize),
    /// 이 자리에서 끝나는 그림 줄 아래에 새 줄을 만들어
    BelowImage(usize),
}

/// 글자 위치 `at` 에 글을 놓을 때 실제로 넣을 자리.
/// 그림 줄은 한 덩어리라 (`guard_image_edges` 와 같은 규칙) 그림 바로 앞이면 위에 새 줄,
/// 그림 줄 안이나 바로 뒤면 아래에 새 줄을 만들어 넣습니다.
/// `images`: 그림 줄들의 (시작 글자, 끝 글자, …)
pub(super) fn drop_spot(at: usize, images: &[(usize, usize, usize)]) -> DropSpot {
    match images.iter().find(|(start, end, _)| (*start..=*end).contains(&at)) {
        Some(&(start, _, _)) if at == start => DropSpot::AboveImage(start),
        Some(&(_, end, _)) => DropSpot::BelowImage(end),
        None => DropSpot::At(at),
    }
}

/// `spot` 에 `text` 를 넣으려면: (넣을 글자 위치, 넣을 글, 넣은 뒤 커서 자리 = 끌어온 글의 끝)
pub(super) fn drop_insertion(spot: DropSpot, text: &str) -> (usize, String, usize) {
    match spot {
        DropSpot::At(at) => (at, text.to_owned(), at + text.chars().count()),
        // 그림 위·아래의 새 줄에는 끝 줄바꿈 없이 (빈 줄이 덧붙지 않게)
        DropSpot::AboveImage(start) => {
            let text = text.trim_end_matches('\n');
            (start, format!("{text}\n"), start + text.chars().count())
        }
        DropSpot::BelowImage(end) => {
            let text = text.trim_end_matches('\n');
            (end, format!("\n{text}"), end + 1 + text.chars().count())
        }
    }
}

impl NotepadApp {
    /// 끌어온 글을 `spot` 에 넣고, 커서를 그 글 끝에 두고 편집기로 돌아갑니다.
    pub(super) fn insert_dropped_text(&mut self, ctx: &egui::Context, id: Id, spot: DropSpot, text: &str) {
        let (at, insert, caret) = drop_insertion(spot, text);
        let doc = &mut self.docs[self.active];
        let byte = markdown::char_to_byte(&doc.text, at);
        doc.text.insert_str(byte, &insert);
        doc.mark_changed();
        set_caret(ctx, id, caret);
        self.selected_image = None;
        self.focus_editor = true;
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` 의 글자 위치 `at` 에 `dropped` 를 놓은 결과와 커서 자리 (`|`)
    fn dropped_at(text: &str, at: usize, images: &[(usize, usize, usize)], dropped: &str) -> String {
        let (pos, insert, caret) = drop_insertion(drop_spot(at, images), dropped);
        let mut out = text.to_owned();
        out.insert_str(markdown::char_to_byte(text, pos), &insert);
        out.insert(markdown::char_to_byte(&out, caret), '|');
        out
    }

    #[test]
    fn text_lands_where_it_is_dropped() {
        let text = "가나다라\n\n끝";
        assert_eq!(dropped_at(text, 2, &[], "ABC"), "가나ABC|다라\n\n끝", "줄 가운데");
        assert_eq!(dropped_at(text, 5, &[], "ABC"), "가나다라\nABC|\n끝", "빈 줄");
        assert_eq!(dropped_at(text, 7, &[], "여러\n줄\n"), "가나다라\n\n끝여러\n줄\n|", "문서 끝");
        assert_eq!(dropped_at("", 0, &[], "ABC"), "ABC|");
    }

    #[test]
    fn image_lines_are_never_split() {
        let text = "위\n![고양이](cat.png)\n아래";
        let image = "![고양이](cat.png)";
        let start = 2;
        let end = start + image.chars().count();
        let images = [(start, end, "위\n".len())];
        let above = "위\nABC|\n![고양이](cat.png)\n아래";
        let below = "위\n![고양이](cat.png)\nABC|\n아래";
        assert_eq!(dropped_at(text, start, &images, "ABC"), above, "그림 바로 앞");
        assert_eq!(dropped_at(text, start + 5, &images, "ABC"), below, "그림 줄 안");
        assert_eq!(dropped_at(text, end, &images, "ABC"), below, "그림 바로 뒤");
        assert_eq!(dropped_at(text, end, &images, "ABC\n"), below, "끝 줄바꿈은 덧붙이지 않음");
        // 그림 줄 밖은 그대로
        assert_eq!(dropped_at(text, 1, &images, "ABC"), "위ABC|\n![고양이](cat.png)\n아래");
        assert_eq!(dropped_at(text, end + 1, &images, "ABC"), "위\n![고양이](cat.png)\nABC|아래");
        // 그림으로 끝나는 문서
        let last = "![고양이](cat.png)";
        let images = [(0, last.chars().count(), 0)];
        assert_eq!(dropped_at(last, 7, &images, "ABC"), "![고양이](cat.png)\nABC|");
        assert_eq!(dropped_at(last, 0, &images, "ABC"), "ABC|\n![고양이](cat.png)");
    }

    #[test]
    fn spot_names_the_image_edge() {
        let images = [(10, 40, 12), (41, 60, 50)];
        assert_eq!(drop_spot(9, &images), DropSpot::At(9));
        assert_eq!(drop_spot(10, &images), DropSpot::AboveImage(10));
        assert_eq!(drop_spot(25, &images), DropSpot::BelowImage(40));
        assert_eq!(drop_spot(40, &images), DropSpot::BelowImage(40));
        assert_eq!(drop_spot(41, &images), DropSpot::AboveImage(41), "그림이 이어지면 둘 사이의 새 줄");
        assert_eq!(drop_spot(61, &images), DropSpot::At(61));
    }
}
