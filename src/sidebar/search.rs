//! 사이드바 검색: 이름 · 내용 찾기와 태그 검색식(AND · OR · 빼기), 검색 결과 목록.

use super::*;

/// 검색 결과
#[derive(Clone, Default)]
pub(super) struct Found {
    notes: Vec<PathBuf>,
    /// 태그 검색일 때: 치고 있는 태그로 시작하는 태그들 (태그, 메모 수)
    tags: Vec<(String, usize)>,
    /// 태그 검색식이었는지
    tag_mode: bool,
}

/// 검색어가 태그 검색식(`#a #b`, `#a OR #b` …)이면 태그로, 아니면 이름·내용으로 찾습니다.
/// 화면을 멈추지 않도록 뒤의 스레드에서 부릅니다.
pub(super) fn search_notes(roots: &[PathBuf], query: &str) -> Found {
    match crate::tags::TagQuery::parse(query) {
        Some(tq) => search_tags(roots, &tq),
        None => Found { notes: search_text(roots, query), ..Default::default() },
    }
}

/// 메모 내용 (열 때와 똑같이 인코딩을 알아내 읽음, 너무 큰 파일은 건너뜀)
pub(super) fn note_text(p: &Path) -> Option<String> {
    if std::fs::metadata(p).ok()?.len() >= 512 * 1024 {
        return None;
    }
    Some(crate::encoding::decode(&std::fs::read(p).ok()?).0)
}

/// 태그 검색: 검색식에 맞는 메모(이름 순, 최대 500개)와, 치고 있는 태그로 시작하는 태그(많이 쓴 순, 최대 40개)
pub(super) fn search_tags(roots: &[PathBuf], tq: &crate::tags::TagQuery) -> Found {
    let mut notes = Vec::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut scanned = 0usize;
    for root in roots {
        walk(root, 0, &mut |p| {
            scanned += 1;
            if scanned > 20_000 || !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
                return;
            }
            let Some(text) = note_text(p) else { return };
            let tags = crate::tags::extract(&text);
            for t in &tags {
                *counts.entry(t.clone()).or_default() += 1;
            }
            if tq.matches(&tags) {
                notes.push(p.to_path_buf());
            }
        });
    }
    notes.sort_by(|a, b| natural_cmp(&display_name(a, false), &display_name(b, false)));
    notes.truncate(500);
    let mut tags: Vec<(String, usize)> = match &tq.typing {
        Some(prefix) => counts.into_iter().filter(|(t, _)| t.starts_with(prefix.as_str())).collect(),
        None => Vec::new(),
    };
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    tags.truncate(40);
    Found { notes, tags, tag_mode: true }
}

/// 메모 폴더들에서 이름 또는 내용에 `query`(소문자)가 들어간 메모. 이름이 맞는 것부터, 최대 200개.
pub(super) fn search_text(roots: &[PathBuf], query: &str) -> Vec<PathBuf> {
    let mut by_name = Vec::new();
    let mut by_content = Vec::new();
    let mut scanned = 0usize;
    for root in roots {
        walk(root, 0, &mut |p| {
            scanned += 1;
            if scanned > 20_000 || by_name.len() + by_content.len() >= 200 {
                return;
            }
            if display_name(p, false).to_lowercase().contains(query) {
                by_name.push(p.to_path_buf());
            } else if note_text(p).is_some_and(|t| t.to_lowercase().contains(query)) {
                by_content.push(p.to_path_buf());
            }
        });
    }
    by_name.extend(by_content);
    by_name.truncate(200);
    by_name
}

impl Sidebar {
    pub(super) fn search_results(
        &mut self,
        ui: &mut Ui,
        roots: &[PathBuf],
        active: Option<&Path>,
        actions: &mut Vec<Action>,
        k: Colors,
    ) {
        let q = self.query.trim().to_lowercase();
        // 뒤에서 끝난 검색 결과 받기
        if let Some((jq, rx)) = &self.search_job {
            match rx.try_recv() {
                Ok(found) => {
                    self.search = Some((jq.clone(), Instant::now(), found));
                    self.search_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.search_job = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        // 검색어가 바뀌었거나 결과가 3초 넘게 지났으면 다시 (이미 돌고 있으면 그게 끝난 뒤에)
        let stale = self.search.as_ref().is_none_or(|(sq, t, _)| *sq != q || t.elapsed() > Duration::from_secs(3));
        if stale && self.search_job.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let (roots, query, ctx) = (roots.to_vec(), q.clone(), ui.ctx().clone());
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let found = search_notes(&roots, &query);
                crate::perf::record("search", started.elapsed());
                crate::perf::set_note("search_last", format!("'{}' → {}개", query, found.notes.len()));
                let _ = tx.send(found);
                ctx.request_repaint();
            });
            self.search_job = Some((q.clone(), rx));
        }
        let searching = self.search_job.is_some() && self.search.as_ref().is_none_or(|(sq, _, _)| *sq != q);
        if searching {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let found = self.search.as_ref().map(|(_, _, r)| r.clone()).unwrap_or_default();
        // 태그 검색: 치고 있는 태그로 시작하는 태그들 (누르면 그 태그로 채움)
        if !found.tags.is_empty() {
            section_label(ui, t!("태그"), k);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                ui.add_space(8.0);
                for (tag, count) in &found.tags {
                    let chip = egui::Button::new(RichText::new(format!("#{tag}  {count}")).size(12.5).color(k.accent))
                        .fill(mix(k.panel, k.accent, 0.10))
                        .corner_radius(10);
                    if ui.add(chip).clicked() {
                        self.query = crate::tags::complete_last(&self.query, tag);
                        ui.memory_mut(|m| m.request_focus(egui::Id::new("sidebar_search")));
                    }
                }
            });
            ui.add_space(6.0);
        }
        let results = found.notes;
        let tag_query_empty = found.tag_mode && crate::tags::TagQuery::parse(&q).is_none_or(|t| t.is_empty());
        if searching {
            section_label(ui, t!("찾는 중…"), k);
        } else if tag_query_empty {
            if found.tags.is_empty() {
                hint_label(ui, t!("아직 태그가 없습니다. 글에 #태그 처럼 쓰면 됩니다."), k);
            }
            hint_label(ui, t!("#a #b 는 둘 다, #a OR #b 는 둘 중 하나, -#a 는 빼기"), k);
        } else if found.tag_mode {
            section_label(ui, &tf!("태그가 맞는 메모 {}개", results.len()), k);
        } else {
            section_label(ui, &tf!("검색 결과 {}개", results.len()), k);
        }
        for p in results {
            // 숨긴 메모는 검색에도 (보이기를 켜지 않았으면) 나오지 않음
            let hidden = self.is_hidden_path(roots, &p);
            if hidden && !self.show_hidden {
                continue;
            }
            let name = display_name(&p, false);
            let mark = RowMark { dim: hidden, ..Default::default() };
            let resp = tree_row(ui, 0, RowIcon::Note, &name, active == Some(p.as_path()), mark, k);
            let folder = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
            let resp = resp.on_hover_text(folder);
            if resp.clicked() {
                actions.push(Action::Open(p.clone()));
            }
            resp.context_menu(|ui| {
                if ui.button(t!("열기")).clicked() {
                    actions.push(Action::Open(p.clone()));
                    ui.close();
                }
                if ui.button(t!("하이퍼링크 복사")).clicked() {
                    actions.push(Action::CopyLink(p.clone()));
                    ui.close();
                }
                if ui.button(t!("탐색기에서 보기")).clicked() {
                    actions.push(Action::Reveal(p.clone()));
                    ui.close();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_search_with_and_or() {
        let root = std::env::temp_dir().join(format!("tbn_tags_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("하위")).unwrap();
        std::fs::write(root.join("가.md"), "회의 #회의 #예산").unwrap();
        std::fs::write(root.join("하위/나.md"), "#회의 만").unwrap();
        std::fs::write(root.join("다.md"), "#여행 #예산").unwrap();
        std::fs::write(root.join("라.txt"), "#회의 (txt 는 태그 없음)").unwrap();
        let names = |q: &str| -> Vec<String> {
            search_notes(std::slice::from_ref(&root), q).notes.iter().map(|p| display_name(p, false)).collect()
        };
        assert_eq!(names("#회의"), ["가", "나"]);
        assert_eq!(names("#회의 #예산"), ["가"]);
        assert_eq!(names("#회의 or #여행"), ["가", "나", "다"]);
        assert_eq!(names("#예산 -#여행"), ["가"]);
        let found = search_notes(std::slice::from_ref(&root), "#");
        assert!(found.tag_mode && found.notes.is_empty());
        assert_eq!(found.tags[..2], [("예산".to_string(), 2), ("회의".to_string(), 2)]);
        let found = search_notes(std::slice::from_ref(&root), "#여");
        assert_eq!(found.tags, [("여행".to_string(), 1)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_reads_legacy_encodings() {
        let root = std::env::temp_dir().join(format!("tbn_search_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("회의록\r\n예산 검토는 다음 주에 합니다.\r\n");
        std::fs::write(root.join("옛날.txt"), &cp949).unwrap();
        let utf16: Vec<u8> =
            [0xFF, 0xFE].into_iter().chain("예산 메모".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        std::fs::write(root.join("유니코드.txt"), utf16).unwrap();
        std::fs::write(root.join("다른 것.md"), "관계없는 글").unwrap();
        let mut found: Vec<_> = search_notes(std::slice::from_ref(&root), "예산")
            .notes
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, ["옛날.txt", "유니코드.txt"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
