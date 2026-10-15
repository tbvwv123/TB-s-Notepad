//! 시스템 글꼴 목록과 egui 글꼴 구성.
//!
//! 글꼴 파일은 필요할 때(선택했을 때)만 읽어서 시작을 가볍게 유지합니다.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

#[derive(Clone, Debug)]
pub struct SystemFont {
    pub name: String,
    pub path: PathBuf,
    /// .ttc(글꼴 모음) 안에서의 순번
    pub index: u32,
}

#[derive(Default)]
pub struct FontLibrary {
    /// 고를 수 있는 글꼴 (굵게/기울임 같은 변형은 뺀 목록)
    pub fonts: Vec<SystemFont>,
    /// 굵은 글꼴을 찾을 때 쓰는 전체 목록
    all: Vec<SystemFont>,
    /// 읽어 둔 글꼴 파일. 프로그램이 끝날 때까지 쓰므로 `'static`으로 붙잡아 둡니다
    /// (같은 글꼴을 여러 글꼴 묶음에 넣어도 복사하지 않도록).
    loaded: HashMap<PathBuf, &'static [u8]>,
    /// 본문 글꼴에 굵은 글꼴이 없어서 글자를 겹쳐 그려 굵게 보이게 하는지 (가짜 굵게)
    pub fake_bold: bool,
    /// 글꼴마다 한글이 들어 있는지 (설정 창 안내용, 한 번 확인하면 기억)
    hangul: HashMap<String, bool>,
}

/// 본문 글꼴의 굵은 변형을 찾을 때 붙여 보는 이름 (앞에 있는 것부터)
const BOLD_SUFFIXES: &[&str] = &["Bold", "SemiBold", "Semibold", "DemiBold", "ExtraBold", "Heavy", "Black"];

/// 한글(+ 한자/가나) 대체 글꼴 후보. 메모리를 아끼려고 처음 찾은 하나만 씁니다.
const CJK_FALLBACKS: &[&str] = &["Malgun Gothic", "Noto Sans CJK KR", "NanumGothic", "Apple SD Gothic Neo"];

/// ☑ ❝ ▦ 같은 기호용 대체 글꼴
const SYMBOL_FALLBACKS: &[&str] = &["Segoe UI Symbol"];

const FALLBACK_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
];

const PREFERRED_EDITOR_FONTS: &[&str] = &["D2Coding", "Cascadia Mono", "Consolas"];

pub const EDITOR_FAMILY: &str = "editor";

pub fn editor_family() -> FontFamily {
    FontFamily::Name(EDITOR_FAMILY.into())
}

/// 문서 모드 본문
pub fn doc_family() -> FontFamily {
    FontFamily::Name("doc".into())
}

/// 문서 모드 굵은 글씨 (제목, **굵게**)
pub fn bold_family() -> FontFamily {
    FontFamily::Name("doc-bold".into())
}

impl FontLibrary {
    pub fn load() -> Self {
        let mut all = enumerate_system_fonts();
        all.sort_by_key(|f| f.name.to_lowercase());
        all.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        let fonts = all.iter().filter(|f| !is_style_variant(&f.name)).cloned().collect();
        Self { fonts, all, loaded: HashMap::new(), fake_bold: false, hangul: HashMap::new() }
    }

    /// 이 글꼴에 한글(가)이 들어 있는지. 읽을 수 없으면 `None`
    pub fn has_hangul(&mut self, name: &str) -> Option<bool> {
        if let Some(v) = self.hangul.get(name) {
            return Some(*v);
        }
        let data = self.data_by_name(name)?;
        let font = skrifa::FontRef::from_index(&data.font, data.index).ok()?;
        let v = skrifa::MetadataProvider::charmap(&font).map('가').is_some();
        self.hangul.insert(name.to_owned(), v);
        Some(v)
    }

    /// 글꼴의 굵은 변형 이름. "Pretendard Regular" → "Pretendard Bold",
    /// "에스코어 드림 4 Regular" → "에스코어 드림 6 Bold" 처럼 이름 끝의 Regular · 숫자를 떼고도 찾아봅니다.
    pub fn bold_variant(&self, name: &str) -> Option<String> {
        let mut bases = vec![name.to_owned()];
        let mut words: Vec<&str> = name.split_whitespace().collect();
        while words.len() > 1
            && let Some(last) = words.last()
            && (["regular", "normal", "book", "roman", "text"].contains(&last.to_lowercase().as_str())
                || last.chars().all(|c| c.is_ascii_digit()))
        {
            words.pop();
            bases.push(words.join(" "));
        }
        // 1) 정확한 이름
        for base in &bases {
            for suffix in BOLD_SUFFIXES {
                let n = format!("{base} {suffix}");
                if self.find(&n).is_some() {
                    return Some(n);
                }
            }
        }
        // 2) 같은 이름으로 시작하고 굵기 낱말이 붙은 것 ("에스코어 드림 6 Bold")
        let base = bases.last()?.to_lowercase();
        for suffix in BOLD_SUFFIXES {
            let suffix = suffix.to_lowercase();
            let hit = self.all.iter().find(|f| {
                let lower = f.name.to_lowercase();
                lower.starts_with(&format!("{base} "))
                    && lower[base.len()..].split_whitespace().any(|w| w == suffix)
                    && lower[base.len()..]
                        .split_whitespace()
                        .all(|w| w == suffix || w.chars().all(|c| c.is_ascii_digit()))
            });
            if let Some(f) = hit {
                return Some(f.name.clone());
            }
        }
        None
    }

    /// 가변 글꼴이면 굵기 축(wght)을 `weight` 로 맞춘 복사본
    fn with_weight(data: &Arc<FontData>, weight: f32) -> Option<Arc<FontData>> {
        let font = skrifa::FontRef::from_index(&data.font, data.index).ok()?;
        let axes = skrifa::MetadataProvider::axes(&font);
        let axis = axes.iter().find(|a| a.tag() == skrifa::Tag::new(b"wght"))?;
        let w = weight.clamp(axis.min_value(), axis.max_value());
        let mut d = (**data).clone();
        d.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", w)]);
        Some(Arc::new(d))
    }

    pub fn find(&self, name: &str) -> Option<&SystemFont> {
        self.all.iter().find(|f| f.name.eq_ignore_ascii_case(name))
    }

    fn data(&mut self, font: &SystemFont) -> Option<Arc<FontData>> {
        let bytes = match self.loaded.get(&font.path) {
            Some(b) => *b,
            None => {
                let b: &'static [u8] = Box::leak(std::fs::read(&font.path).ok()?.into_boxed_slice());
                self.loaded.insert(font.path.clone(), b);
                b
            }
        };
        em_metrics(bytes, font.index)?; // 읽을 수 없는 글꼴은 건너뜀
        let mut data = FontData::from_static(bytes);
        data.index = font.index;
        Some(Arc::new(data))
    }

    fn data_by_name(&mut self, name: &str) -> Option<Arc<FontData>> {
        let font = self.find(name)?.clone();
        self.data(&font)
    }

    /// 설정에 맞는 글꼴 정의를 만듭니다.
    pub fn definitions(
        &mut self,
        editor_font: Option<&str>,
        ui_font: Option<&str>,
        doc_font: Option<&str>,
    ) -> FontDefinitions {
        let mut defs = FontDefinitions::default();

        // 대체 글꼴 (한글 등)
        let mut fallbacks = Vec::new();
        for group in [CJK_FALLBACKS, SYMBOL_FALLBACKS] {
            for name in group {
                if let Some(data) = self.data_by_name(name) {
                    let key = format!("fallback:{name}");
                    defs.font_data.insert(key.clone(), data);
                    fallbacks.push(key);
                    break;
                }
            }
        }
        if !fallbacks.iter().any(|k| CJK_FALLBACKS.iter().any(|n| k.ends_with(n))) {
            for path in FALLBACK_PATHS {
                let font = SystemFont { name: path.to_string(), path: path.into(), index: 0 };
                if let Some(data) = self.data(&font) {
                    let key = format!("fallback:{path}");
                    defs.font_data.insert(key.clone(), data);
                    fallbacks.push(key);
                    break;
                }
            }
        }

        let default_prop = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        let default_mono = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();

        // 화면 글꼴: 지정한 글꼴 → (없으면) 대체 글꼴 중 첫 번째(맑은 고딕) → egui 기본
        let mut prop = Vec::new();
        if let Some(data) = ui_font.and_then(|n| self.data_by_name(n)) {
            defs.font_data.insert("ui".to_owned(), data);
            prop.push("ui".to_owned());
        } else if let Some(first) = fallbacks.first() {
            prop.push(first.clone());
        }
        prop.extend(default_prop.iter().cloned());
        prop.extend(fallbacks.iter().cloned());
        dedup(&mut prop);
        defs.families.insert(FontFamily::Proportional, prop);

        let mut mono = default_mono.clone();
        mono.extend(fallbacks.iter().cloned());
        dedup(&mut mono);
        defs.families.insert(FontFamily::Monospace, mono);

        // 편집기 글꼴
        let mut editor = Vec::new();
        let chosen = editor_font
            .map(str::to_owned)
            .or_else(|| PREFERRED_EDITOR_FONTS.iter().find(|n| self.find(n).is_some()).map(|s| s.to_string()));
        if let Some(data) = chosen.and_then(|n| self.data_by_name(&n)) {
            defs.font_data.insert("editor".to_owned(), data);
            editor.push("editor".to_owned());
        }
        editor.extend(default_mono.iter().cloned());
        editor.extend(fallbacks.iter().cloned());
        dedup(&mut editor);
        defs.families.insert(editor_family(), editor);

        // 문서 본문과 굵은 글씨 (제목, **굵게**)
        // 굵은 글씨는 고른 글꼴의 굵은 변형 → 가변 글꼴이면 굵기 축 → 둘 다 없으면 같은 글꼴을 겹쳐 그려 굵게(가짜 굵게).
        // 예전처럼 다른 글꼴(맑은 고딕 굵게)로 바꾸지 않아서, 제목도 고른 글꼴 모양 그대로 나옵니다.
        let doc_name = doc_font.unwrap_or("Malgun Gothic").to_owned();
        let doc_data = self.data_by_name(&doc_name);
        let mut doc = Vec::new();
        if let Some(data) = &doc_data {
            let data = Self::with_weight(data, 400.0).unwrap_or_else(|| data.clone());
            defs.font_data.insert("doc".to_owned(), data);
            doc.push("doc".to_owned());
        }
        doc.extend(fallbacks.iter().cloned());
        doc.extend(default_prop.iter().cloned());
        dedup(&mut doc);

        self.fake_bold = false;
        let mut bold = Vec::new();
        let variant = self.bold_variant(&doc_name).and_then(|n| self.data_by_name(&n));
        let real_bold = variant.or_else(|| doc_data.as_ref().and_then(|d| Self::with_weight(d, 700.0)));
        match (real_bold, &doc_data) {
            (Some(data), _) => {
                defs.font_data.insert("bold".to_owned(), data);
                bold.push("bold".to_owned());
                // 그 글꼴에 한글이 없을 때 한글도 굵게 보이도록 굵은 한글 대체 글꼴을 먼저
                if let Some(data) = self.data_by_name("Malgun Gothic Bold") {
                    let key = "fallback:Malgun Gothic Bold".to_owned();
                    defs.font_data.insert(key.clone(), data);
                    bold.push(key);
                }
                bold.extend(doc.iter().cloned());
            }
            (None, Some(_)) => {
                // 같은 글꼴 그대로 (편집기가 겹쳐 그려 굵게 보이게 함)
                self.fake_bold = true;
                bold.extend(doc.iter().cloned());
            }
            (None, None) => {
                // 본문 글꼴을 못 읽었으면 예전처럼 맑은 고딕 굵게
                for name in ["Malgun Gothic Bold", "Segoe UI Bold"] {
                    if let Some(data) = self.data_by_name(name) {
                        let key = format!("bold:{name}");
                        defs.font_data.insert(key.clone(), data);
                        bold.push(key);
                        break;
                    }
                }
                bold.extend(doc.iter().cloned());
            }
        }
        dedup(&mut bold);
        defs.families.insert(doc_family(), doc);
        defs.families.insert(bold_family(), bold);

        align_fallbacks(&mut defs, FontFamily::Proportional, "prop");
        align_fallbacks(&mut defs, doc_family(), "doc");
        align_fallbacks(&mut defs, bold_family(), "bold");
        align_fallbacks(&mut defs, FontFamily::Monospace, "mono");
        align_fallbacks(&mut defs, editor_family(), "editor");
        defs
    }
}

/// 가짜 굵게: 글자를 오른쪽으로 몇 픽셀 겹쳐 그릴 거리들 (화면 점 단위).
/// 글자가 클수록 더 두껍게, 흐려지지 않도록 화면 픽셀 단위로 옮깁니다.
pub fn fake_bold_offsets(font_size: f32, pixels_per_point: f32) -> Vec<f32> {
    let px = (font_size * 0.05 * pixels_per_point).round().clamp(1.0, 3.0) as usize;
    (1..=px).map(|k| k as f32 / pixels_per_point).collect()
}

/// 가짜 굵게용: 이 글자 배치에서 굵은 글씨만 남기고 나머지는 투명하게 만든 복사본.
/// 원래 배치와 글꼴·크기가 같아서 글자 자리가 똑같습니다. 굵은 글씨가 없으면 `None`.
pub fn bold_only_job(job: &egui::text::LayoutJob) -> Option<egui::text::LayoutJob> {
    let bold = bold_family();
    let mut out = job.clone();
    let mut any = false;
    for s in &mut out.sections {
        if s.format.font_id.family == bold && s.format.color.a() > 0 {
            any = true;
            s.format.background = egui::Color32::TRANSPARENT;
        } else {
            s.format.color = egui::Color32::TRANSPARENT;
            s.format.background = egui::Color32::TRANSPARENT;
            s.format.underline = egui::Stroke::NONE;
            s.format.strikethrough = egui::Stroke::NONE;
        }
    }
    any.then_some(out)
}

/// (ascent, 줄 높이) — 글꼴 크기(em) 대비 비율
fn em_metrics(data: &[u8], index: u32) -> Option<(f32, f32)> {
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let m = skrifa::MetadataProvider::metrics(
        &font,
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    let upem = m.units_per_em as f32;
    (upem > 0.0).then(|| (m.ascent / upem, (m.ascent - m.descent + m.leading) / upem))
}

/// egui는 글꼴을 섞을 때 높이의 가운데를 맞추므로, 한글처럼 위아래 여백이 큰 대체 글꼴은
/// 영문보다 아래로 처집니다. 기준선이 같아지도록 세로 위치를 보정합니다.
///
/// 편집기는 첫 줄 위쪽을 잘라내므로 한글을 위로 올리지 않고, 대신 첫 글꼴(영문)을 아래로
/// 내립니다. 줄 간격의 여유는 글자 아래쪽에 있어서 내려도 잘리지 않습니다.
fn align_fallbacks(defs: &mut FontDefinitions, family: FontFamily, tag: &str) {
    let Some(list) = defs.families.get(&family).cloned() else { return };
    let Some(primary) = list.first().and_then(|k| defs.font_data.get(k)).cloned() else { return };
    let Some((pa, ph)) = em_metrics(&primary.font, primary.index) else { return };

    // 각 대체 글꼴이 첫 글꼴 기준선에 맞으려면 움직여야 할 양 (+ 아래, - 위)
    // 이 글꼴의 기준선 = fa + (ph - fh) / 2  →  pa 와 같아지도록
    let offsets: Vec<Option<f32>> = list
        .iter()
        .enumerate()
        .map(|(i, key)| {
            if i == 0 || !key.starts_with("fallback:") {
                return None;
            }
            let d = defs.font_data.get(key)?;
            let (fa, fh) = em_metrics(&d.font, d.index)?;
            Some(pa - fa - 0.5 * (ph - fh))
        })
        .collect();
    // 가장 많이 쓰는 첫 번째 대체 글꼴(한글)이 제자리에 있도록 첫 글꼴을 그만큼 내림
    let shift = offsets.iter().flatten().next().map_or(0.0, |o| (-o).max(0.0));

    let mut new_list = Vec::with_capacity(list.len());
    for (i, key) in list.iter().enumerate() {
        let offset = if i == 0 { (shift > 0.0).then_some(shift) } else { offsets[i].map(|o| o + shift) };
        match (offset, defs.font_data.get(key).cloned()) {
            (Some(offset), Some(data)) if offset.abs() > 0.001 => {
                let mut aligned = (*data).clone();
                aligned.tweak.y_offset_factor = offset;
                let new_key = format!("{key}@{tag}");
                defs.font_data.insert(new_key.clone(), Arc::new(aligned));
                new_list.push(new_key);
            }
            _ => new_list.push(key.clone()),
        }
    }
    defs.families.insert(family, new_list);
}

fn dedup(v: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    v.retain(|s| seen.insert(s.clone()));
}

/// 굵게/기울임 등 스타일 변형은 목록에서 뺍니다 (편집기 글꼴은 보통체만 사용).
fn is_style_variant(name: &str) -> bool {
    const STYLES: &[&str] = &[
        "bold",
        "italic",
        "oblique",
        "light",
        "semilight",
        "semibold",
        "demibold",
        "black",
        "thin",
        "medium",
        "heavy",
        "extrabold",
        "extralight",
        "ultralight",
        "condensed",
    ];
    name.split_whitespace().skip(1).any(|w| STYLES.contains(&w.to_lowercase().as_str()))
}

#[cfg(windows)]
fn enumerate_system_fonts() -> Vec<SystemFont> {
    use winreg::types::FromRegValue;

    let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
    let system_dir = windir.join("Fonts");
    let mut out = Vec::new();

    for root in [winreg::HKLM, winreg::HKCU] {
        let Ok(key) = root.open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts") else {
            continue;
        };
        for (value_name, value) in key.enum_values().flatten() {
            let Ok(file) = String::from_reg_value(&value) else { continue };
            let lower = file.to_lowercase();
            if !(lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".ttc")) {
                continue;
            }
            let path = {
                let p = PathBuf::from(&file);
                if p.is_absolute() { p } else { system_dir.join(p) }
            };
            // "Cambria & Cambria Math (TrueType)" → ["Cambria", "Cambria Math"]
            let names = match value_name.rsplit_once(" (") {
                Some((n, rest)) if rest.ends_with(')') => n,
                _ => value_name.as_str(),
            };
            for (index, name) in names.split(" & ").enumerate() {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                out.push(SystemFont { name: name.to_owned(), path: path.clone(), index: index as u32 });
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn enumerate_system_fonts() -> Vec<SystemFont> {
    let mut dirs: Vec<PathBuf> = vec![
        "/usr/share/fonts".into(),
        "/usr/local/share/fonts".into(),
        "/System/Library/Fonts".into(),
        "/Library/Fonts".into(),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".fonts"));
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join("Library/Fonts"));
    }
    let mut out = Vec::new();
    let mut stack = dirs;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                let name = path.file_stem().unwrap_or_default().to_string_lossy().replace(['-', '_'], " ");
                out.push(SystemFont { name, path, index: 0 });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_bold_offsets_scale_with_size() {
        assert_eq!(fake_bold_offsets(16.0, 1.0), vec![1.0]);
        assert_eq!(fake_bold_offsets(40.0, 1.0), vec![1.0, 2.0]);
        assert_eq!(fake_bold_offsets(16.0, 2.0), vec![0.5, 1.0]);
        assert_eq!(fake_bold_offsets(500.0, 1.0).len(), 3);
    }

    #[test]
    fn bold_only_job_hides_everything_else() {
        let mut job = egui::text::LayoutJob::default();
        let fmt = |family: FontFamily| egui::TextFormat {
            font_id: egui::FontId::new(16.0, family),
            color: egui::Color32::RED,
            ..Default::default()
        };
        job.append("보통 ", 0.0, fmt(doc_family()));
        assert!(bold_only_job(&job).is_none());
        job.append("굵게", 0.0, fmt(bold_family()));
        let out = bold_only_job(&job).unwrap();
        assert_eq!(out.text, job.text, "글자 자리는 그대로");
        assert_eq!(out.sections[0].format.color, egui::Color32::TRANSPARENT);
        assert_eq!(out.sections[1].format.color, egui::Color32::RED);
    }
}

#[cfg(test)]
mod survey {
    use super::*;

    /// 이 PC의 글꼴을 모두 읽어 보고 결과를 출력 (cargo test font_survey -- --ignored --nocapture)
    #[test]
    #[ignore]
    fn font_survey() {
        let mut lib = FontLibrary::load();
        let names: Vec<String> = lib.fonts.iter().map(|f| f.name.clone()).collect();
        let (mut unreadable, mut fake, mut real, mut variable) = (vec![], vec![], 0, vec![]);
        for n in &names {
            let Some(d) = lib.data_by_name(n) else {
                unreadable.push(n.clone());
                continue;
            };
            if lib.bold_variant(n).is_some() {
                real += 1;
            } else if FontLibrary::with_weight(&d, 700.0).is_some() {
                variable.push(n.clone());
            } else {
                fake.push(n.clone());
            }
        }
        println!("total {} / real bold {real} / variable {variable:?}", names.len());
        println!("unreadable {unreadable:?}");
        println!("fake bold ({}) {fake:?}", fake.len());
    }
}
