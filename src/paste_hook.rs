//! Ctrl+V 로 이미지 붙여넣기.
//!
//! egui-winit은 Ctrl+V를 받으면 클립보드의 *텍스트*만 확인하고, 텍스트가 없으면 아무 이벤트도
//! 남기지 않고 키를 삼켜 버립니다. 그래서 앱은 Ctrl+V가 눌렸는지조차 알 수 없습니다.
//! Windows에서는 창을 서브클래싱해 winit보다 먼저 `WM_KEYDOWN`을 보고,
//! "텍스트는 없고 이미지/파일이 있는" 클립보드에서 Ctrl+V가 눌렸을 때만 앱에 알려 줍니다.
//! (텍스트가 있으면 지금처럼 egui가 텍스트로 붙여넣습니다.)

use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);

/// 지난 프레임 이후 "이미지 붙여넣기" 요청이 있었는지 (읽으면 초기화)
pub fn take_request() -> bool {
    REQUESTED.swap(false, Ordering::SeqCst)
}

#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;
    use std::sync::atomic::Ordering;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_INSERT, VK_MENU, VK_SHIFT};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_KEYDOWN;

    // 표준 클립보드 형식 번호
    const CF_BITMAP: u32 = 2;
    const CF_DIB: u32 = 8;
    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;
    const CF_DIBV5: u32 = 17;

    static CTX: OnceLock<egui::Context> = OnceLock::new();

    pub fn install(cc: &eframe::CreationContext<'_>) {
        let Ok(handle) = cc.window_handle() else { return };
        let RawWindowHandle::Win32(win) = handle.as_raw() else { return };
        let hwnd = win.hwnd.get() as HWND;
        let _ = CTX.set(cc.egui_ctx.clone());
        // SAFETY: 이 창을 만든 UI 스레드에서 호출하며, 콜백은 정적 함수입니다.
        unsafe {
            SetWindowSubclass(hwnd, Some(subclass_proc), 0x7B_7B, 0);
        }
    }

    fn key_down(vk: u16) -> bool {
        // SAFETY: 단순 조회
        unsafe { GetKeyState(vk as i32) < 0 }
    }

    fn clipboard_has_image_but_no_text() -> bool {
        // SAFETY: 클립보드를 열지 않는 단순 조회 함수들입니다.
        unsafe {
            if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
                return false;
            }
            let png: Vec<u16> = "PNG\0".encode_utf16().collect();
            let png_format = RegisterClipboardFormatW(png.as_ptr());
            [CF_DIB, CF_DIBV5, CF_BITMAP, CF_HDROP, png_format].iter().any(|&f| f != 0 && IsClipboardFormatAvailable(f) != 0)
        }
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == WM_KEYDOWN {
            let ctrl_v = wparam == b'V' as usize && key_down(VK_CONTROL) && !key_down(VK_MENU);
            let shift_ins = wparam == VK_INSERT as usize && key_down(VK_SHIFT) && !key_down(VK_CONTROL);
            if (ctrl_v || shift_ins) && clipboard_has_image_but_no_text() {
                super::REQUESTED.store(true, Ordering::SeqCst);
                if let Some(ctx) = CTX.get() {
                    ctx.request_repaint();
                }
            }
        }
        // SAFETY: 처리 여부와 관계없이 원래 창 프로시저(winit)로 넘깁니다.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

/// 창이 만들어진 직후 한 번 호출합니다.
pub fn install(cc: &eframe::CreationContext<'_>) {
    #[cfg(windows)]
    imp::install(cc);
    #[cfg(not(windows))]
    let _ = cc;
}
