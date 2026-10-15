// 릴리스 빌드에서는 콘솔 창을 띄우지 않습니다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod document;
mod fonts;
mod live;
mod markdown;
mod mcp;
mod paste_hook;
mod settings;
mod sidebar;
mod ui_kit;

use std::path::PathBuf;

/// 설정 저장 폴더 이름 등에 쓰는 식별자
pub const APP_ID: &str = "tb-notepad";

/// 설정·세션을 저장할 폴더.
/// - 환경 변수 `TB_NOTEPAD_DATA`가 있으면 그 폴더
/// - 실행 파일 옆에 `portable.txt`가 있으면 실행 파일 옆 `data` 폴더 (USB 등에 들고 다니는 포터블 모드)
/// - 아니면 `%APPDATA%	b-notepad\data`
pub fn data_dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        if let Some(dir) = std::env::var_os("TB_NOTEPAD_DATA") {
            return PathBuf::from(dir);
        }
        if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from))
            && exe_dir.join("portable.txt").exists()
        {
            return exe_dir.join("data");
        }
        eframe::storage_dir(APP_ID).unwrap_or_else(|| std::env::temp_dir().join(APP_ID))
    })
    .clone()
}

fn main() -> eframe::Result {
    // `--mcp`: 창 없이 AI 프로그램(Claude 등)과 stdio로 대화하는 메모 서버 (src/mcp.rs)
    if std::env::args_os().skip(1).any(|a| a == "--mcp") {
        mcp::run();
        return Ok(());
    }

    // 명령줄로 받은 파일 (탐색기의 "연결 프로그램"으로 열 때 등)
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.is_file()).collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TB's Notepad")
            .with_app_id(APP_ID)
            .with_inner_size([1000.0, 720.0])
            .with_min_inner_size([360.0, 240.0])
            .with_drag_and_drop(true)
            .with_icon(make_icon()),
        persistence_path: Some(data_dir().join("app.ron")),
        ..Default::default()
    };

    eframe::run_native("TB's Notepad", options, Box::new(|cc| Ok(Box::new(app::NotepadApp::new(cc, files)))))
}



/// 코드로 그린 창 아이콘: 둥근 메모지 + 줄 + 연필 색 띠
fn make_icon() -> egui::IconData {
    const N: usize = 64;
    let mut rgba = vec![0u8; N * N * 4];
    let inside_rounded = |x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        let cx = x.clamp(x0 + r, x1 - r);
        let cy = y.clamp(y0 + r, y1 - r);
        let (dx, dy) = (x - cx, y - cy);
        x >= x0 && x <= x1 && y >= y0 && y <= y1 && dx * dx + dy * dy <= r * r
    };
    for py in 0..N {
        for px in 0..N {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            let mut col: Option<[u8; 4]> = None;
            if inside_rounded(x, y, 8.0, 4.0, 56.0, 60.0, 8.0) {
                col = Some([250, 248, 240, 255]);
                // 윗부분 띠
                if y < 16.0 {
                    col = Some([47, 111, 235, 255]);
                }
                // 줄
                for line_y in [26.0, 35.0, 44.0] {
                    if (y - line_y).abs() < 1.6 && (16.0..=48.0).contains(&x) {
                        col = Some([150, 160, 175, 255]);
                    }
                }
                if (y - 53.0).abs() < 1.6 && (16.0..=34.0).contains(&x) {
                    col = Some([150, 160, 175, 255]);
                }
            }
            if let Some(c) = col {
                let i = (py * N + px) * 4;
                rgba[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    egui::IconData { rgba, width: N as u32, height: N as u32 }
}
