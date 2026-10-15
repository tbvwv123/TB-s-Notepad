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
}

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
        Self { fonts, all, loaded: HashMap::new() }
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
    pub fn definitions(&mut self, editor_font: Option<&str>, ui_font: Option<&str>, doc_font: Option<&str>) -> FontDefinitions {
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

        // 문서 본문과 굵은 글씨
        let doc_name = doc_font.unwrap_or("Malgun Gothic").to_owned();
        let mut doc = Vec::new();
        if let Some(data) = self.data_by_name(&doc_name) {
            defs.font_data.insert("doc".to_owned(), data);
            doc.push("doc".to_owned());
        }
        doc.extend(fallbacks.iter().cloned());
        doc.extend(default_prop.iter().cloned());
        dedup(&mut doc);
        let mut bold = Vec::new();
        for name in [format!("{doc_name} Bold"), "Malgun Gothic Bold".to_owned(), "Segoe UI Bold".to_owned()] {
            if let Some(data) = self.data_by_name(&name) {
                let key = format!("bold:{name}");
                defs.font_data.insert(key.clone(), data);
                bold.push(key);
                break;
            }
        }
        bold.extend(doc.iter().cloned());
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
        "bold", "italic", "oblique", "light", "semilight", "semibold", "demibold", "black", "thin",
        "medium", "heavy", "extrabold", "extralight", "ultralight", "condensed",
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
    let mut dirs: Vec<PathBuf> =
        vec!["/usr/share/fonts".into(), "/usr/local/share/fonts".into(), "/System/Library/Fonts".into(), "/Library/Fonts".into()];
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
