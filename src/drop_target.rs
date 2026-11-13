//! 다른 프로그램에서 끌어온 글 받기 (웹 브라우저에서 선택한 글 등).
//!
//! winit 은 창에 자기 OLE 드롭 타깃을 등록하는데, 파일(`CF_HDROP`)만 받고 글에는 "받지 않음"이라고
//! 답합니다. 그래서 egui 는 글을 끌어온 것을 알 수조차 없습니다.
//! Windows 에서는 창이 만들어진 뒤 그 드롭 타깃을 우리 것으로 바꿉니다: 파일은 winit 이 하던 그대로 받고,
//! 글(`CF_UNICODETEXT`)도 받습니다. 받은 것은 여기 모아 두었다가 `NotepadApp::raw_input_hook` 이
//! 프레임마다 가져가 egui 입력에 넣습니다 ([`take`] → [`feed`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// `DROPEFFECT_NONE` / `DROPEFFECT_COPY`
const EFFECT_NONE: u32 = 0;
const EFFECT_COPY: u32 = 1;

/// 끌어온 것을 무엇으로 받을지
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Files,
    Text,
}

/// 파일과 글이 함께 오면 파일로 받습니다 (탐색기에서 끌어온 파일은 경로 글도 같이 올 수 있음).
pub fn choose_kind(has_files: bool, has_text: bool) -> Option<Kind> {
    if has_files {
        Some(Kind::Files)
    } else if has_text {
        Some(Kind::Text)
    } else {
        None
    }
}

/// `CF_UNICODETEXT` 내용 → 글. 끝 표시(0) 뒤는 버리고, 줄바꿈은 `\n` 하나로 맞춥니다.
pub fn text_from_utf16(units: &[u16]) -> String {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end]).replace("\r\n", "\n").replace('\r', "\n")
}

/// 창 안쪽 픽셀 좌표 → egui 좌표. `pixels_per_point` 는 창 배율 × 화면 확대 (egui-winit 과 같은 계산)
pub fn client_to_points(px: (i32, i32), pixels_per_point: f32) -> egui::Pos2 {
    egui::pos2(px.0 as f32 / pixels_per_point, px.1 as f32 / pixels_per_point)
}

/// 글을 놓을 수 있는 영역 (창 안쪽 픽셀 좌표). 끌고 있는 동안 마우스 모양(복사 / 금지)을 정하는 데 씁니다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zone {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Zone {
    pub fn from_points(rect: egui::Rect, pixels_per_point: f32) -> Self {
        let px = |v: f32| (v * pixels_per_point).round() as i32;
        Self { left: px(rect.left()), top: px(rect.top()), right: px(rect.right()), bottom: px(rect.bottom()) }
    }

    pub fn contains(&self, px: (i32, i32)) -> bool {
        (self.left..self.right).contains(&px.0) && (self.top..self.bottom).contains(&px.1)
    }
}

/// 글을 끌고 있을 때 돌려줄 효과: 본문 위이고 끌어온 쪽이 복사를 허락하면 복사, 아니면 받지 않음.
/// (이동은 받지 않습니다: 받으면 끌어온 쪽 프로그램이 원래 글을 지웁니다.)
pub fn text_effect(allowed: u32, in_zone: bool) -> u32 {
    if in_zone && allowed & EFFECT_COPY != 0 { EFFECT_COPY } else { EFFECT_NONE }
}

/// 드롭 타깃(OLE 콜백)과 앱이 주고받는 상태
#[derive(Debug)]
struct State {
    /// 끌고 있는 파일 (창 위에 있는 동안)
    hovered_files: Vec<PathBuf>,
    /// 놓은 파일 (앱이 가져가면 비움)
    dropped_files: Vec<PathBuf>,
    /// 글을 끌고 있는 자리 (창 안쪽 픽셀)
    text_at: Option<(i32, i32)>,
    /// 놓은 글과 그 자리
    dropped_text: Option<(String, (i32, i32))>,
    /// 글을 끌다가 창 밖으로 나가거나 그만둠
    text_gone: bool,
    /// 앱이 알려 준, 글을 놓을 수 있는 영역
    zone: Option<Zone>,
}

impl State {
    const fn new() -> Self {
        Self {
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            text_at: None,
            dropped_text: None,
            text_gone: false,
            zone: None,
        }
    }

    fn in_zone(&self, px: (i32, i32)) -> bool {
        self.zone.is_some_and(|z| z.contains(px))
    }

    fn take(&mut self) -> Pending {
        Pending {
            hovered_files: self.hovered_files.clone(),
            dropped_files: std::mem::take(&mut self.dropped_files),
            text_at: self.text_at,
            dropped_text: self.dropped_text.take(),
            text_gone: std::mem::take(&mut self.text_gone),
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// 지난 프레임 이후 끌어다 놓은 것 (창 안쪽 픽셀 좌표)
#[derive(Debug, Default, PartialEq)]
pub struct Pending {
    pub hovered_files: Vec<PathBuf>,
    pub dropped_files: Vec<PathBuf>,
    pub text_at: Option<(i32, i32)>,
    pub dropped_text: Option<(String, (i32, i32))>,
    pub text_gone: bool,
}

/// 지난 프레임 이후 끌어다 놓은 것을 가져옵니다 (놓은 것은 한 번만 나옴).
pub fn take() -> Pending {
    lock(&STATE).take()
}

/// 글을 놓을 수 있는 영역을 알려 둡니다 (프레임마다, 받을 수 없을 때는 `None`).
pub fn set_text_zone(zone: Option<Zone>) {
    lock(&STATE).zone = zone;
}

/// 다른 프로그램에서 끌어온 글: 끌고 있는 자리와 놓은 글 (egui 좌표)
#[derive(Debug, Default, PartialEq)]
pub struct TextDrag {
    pub hover: Option<egui::Pos2>,
    pub dropped: Option<(String, egui::Pos2)>,
}

/// 끌어다 놓은 것을 egui 입력에 넣습니다. 파일은 winit 이 하던 대로 `hovered_files` / `dropped_files` 에,
/// 글은 끌고 있는 자리로 포인터를 옮겨 (놓을 자리를 보여 주려고) 그 자리와 놓은 글을 돌려줍니다.
pub fn feed(pending: Pending, pixels_per_point: f32, raw: &mut egui::RawInput) -> TextDrag {
    raw.hovered_files
        .extend(pending.hovered_files.into_iter().map(|p| egui::HoveredFile { path: Some(p), ..Default::default() }));
    raw.dropped_files
        .extend(pending.dropped_files.into_iter().map(|p| Arc::new(DroppedPath(p)) as egui::DroppedFileHandle));

    let mut drag = TextDrag::default();
    if let Some(px) = pending.text_at {
        let pos = client_to_points(px, pixels_per_point);
        raw.events.push(egui::Event::PointerMoved(pos));
        drag.hover = Some(pos);
    }
    if let Some((text, px)) = pending.dropped_text {
        let pos = client_to_points(px, pixels_per_point);
        raw.events.push(egui::Event::PointerMoved(pos));
        drag.dropped = Some((text, pos));
    }
    // 끌다가 창 밖으로 나감: 끄는 동안에는 창이 마우스 이동을 받지 못하므로 직접 알려 줌
    if pending.text_gone && drag == TextDrag::default() {
        raw.events.push(egui::Event::PointerGone);
    }
    drag
}

/// 놓은 파일 (egui-winit 의 것과 같음)
#[derive(Debug)]
struct DroppedPath(PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|err| err.to_string())
    }
}

#[cfg(windows)]
mod imp {
    use std::cell::Cell;
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{DRAGDROP_E_NOTREGISTERED, HWND, POINT, POINTL, S_OK};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::{
        CF_HDROP, CF_UNICODETEXT, DROPEFFECT, IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium,
        RevokeDragDrop,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::Win32::UI::WindowsAndMessaging::{GetPropW, SetForegroundWindow};
    use windows::core::{Interface, Ref, Result, implement, w};

    use super::{EFFECT_COPY, EFFECT_NONE, Kind, State, choose_kind, lock, text_effect, text_from_utf16};

    /// 창에 등록하는 드롭 타깃. OLE 가 창을 만든 UI 스레드에서 부릅니다.
    #[implement(IDropTarget)]
    pub(super) struct DropTarget {
        hwnd: HWND,
        ctx: egui::Context,
        state: &'static Mutex<State>,
        /// 지금 끌고 있는 것 (`DragEnter` 에서 정함)
        kind: Cell<Option<Kind>>,
    }

    impl DropTarget {
        pub(super) fn create(hwnd: HWND, ctx: egui::Context, state: &'static Mutex<State>) -> IDropTarget {
            Self { hwnd, ctx, state, kind: Cell::new(None) }.into()
        }

        /// 화면 좌표 → 창 안쪽 픽셀 좌표
        fn client(&self, pt: &POINTL) -> (i32, i32) {
            let mut p = POINT { x: pt.x, y: pt.y };
            // SAFETY: 살아 있는 창 핸들과 지역 변수의 포인터. 실패하면 좌표를 그대로 둡니다.
            let _ = unsafe { ScreenToClient(self.hwnd, &mut p) };
            (p.x, p.y)
        }
    }

    // 주의: 상태 잠금을 쥔 채로 끌어온 쪽 프로그램(`IDataObject`)이나 창 함수를 부르지 않습니다.
    // 그런 호출은 메시지를 돌릴 수 있어서, 그 사이 프레임이 그려지면 같은 잠금을 다시 잡으려다 멈춥니다.
    #[allow(non_snake_case)]
    impl IDropTarget_Impl for DropTarget_Impl {
        fn DragEnter(
            &self,
            pdataobj: Ref<IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> Result<()> {
            let at = self.client(pt);
            let (files, has_text) = pdataobj.as_ref().map_or((Vec::new(), false), |d| (read_files(d), has_text(d)));
            let kind = choose_kind(!files.is_empty(), has_text);
            self.kind.set(kind);
            let effect = {
                let mut st = lock(self.state);
                match kind {
                    Some(Kind::Files) => {
                        st.hovered_files = files;
                        EFFECT_COPY
                    }
                    Some(Kind::Text) => {
                        st.text_at = Some(at);
                        st.text_gone = false;
                        text_effect(allowed(pdweffect), st.in_zone(at))
                    }
                    None => EFFECT_NONE,
                }
            };
            set_effect(pdweffect, effect);
            self.ctx.request_repaint();
            Ok(())
        }

        fn DragOver(&self, _grfkeystate: MODIFIERKEYS_FLAGS, pt: &POINTL, pdweffect: *mut DROPEFFECT) -> Result<()> {
            let effect = match self.kind.get() {
                Some(Kind::Files) => EFFECT_COPY,
                Some(Kind::Text) => {
                    let at = self.client(pt);
                    let mut st = lock(self.state);
                    let moved = st.text_at.replace(at) != Some(at);
                    let effect = text_effect(allowed(pdweffect), st.in_zone(at));
                    drop(st);
                    if moved {
                        self.ctx.request_repaint();
                    }
                    effect
                }
                None => EFFECT_NONE,
            };
            set_effect(pdweffect, effect);
            Ok(())
        }

        fn DragLeave(&self) -> Result<()> {
            let mut st = lock(self.state);
            match self.kind.take() {
                Some(Kind::Files) => st.hovered_files.clear(),
                Some(Kind::Text) => {
                    st.text_at = None;
                    st.text_gone = true;
                }
                None => {}
            }
            drop(st);
            self.ctx.request_repaint();
            Ok(())
        }

        fn Drop(
            &self,
            pdataobj: Ref<IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> Result<()> {
            let data = pdataobj.as_ref();
            let effect = match self.kind.take() {
                Some(Kind::Files) => {
                    let files = data.map(read_files).unwrap_or_default();
                    let mut st = lock(self.state);
                    st.hovered_files.clear();
                    st.dropped_files.extend(files);
                    EFFECT_COPY
                }
                Some(Kind::Text) => {
                    let at = self.client(pt);
                    let effect = text_effect(allowed(pdweffect), lock(self.state).in_zone(at));
                    let text = if effect == EFFECT_NONE { None } else { data.and_then(read_text) };
                    let mut st = lock(self.state);
                    st.text_at = None;
                    match text {
                        Some(text) => {
                            st.dropped_text = Some((text, at));
                            drop(st);
                            // 놓은 자리에서 바로 이어 쓸 수 있게 창을 앞으로 (끌어온 쪽 프로그램이 앞에 있음)
                            // SAFETY: 살아 있는 창 핸들. 실패해도 문제없습니다.
                            let _ = unsafe { SetForegroundWindow(self.hwnd) };
                            EFFECT_COPY
                        }
                        None => {
                            st.text_gone = true;
                            EFFECT_NONE
                        }
                    }
                }
                None => EFFECT_NONE,
            };
            set_effect(pdweffect, effect);
            self.ctx.request_repaint();
            Ok(())
        }
    }

    /// 끌어온 쪽이 허락한 효과들
    fn allowed(pdweffect: *mut DROPEFFECT) -> u32 {
        // SAFETY: OLE 가 준 포인터 (비어 있을 수 있어 확인)
        unsafe { pdweffect.as_ref() }.map_or(EFFECT_NONE, |e| e.0)
    }

    fn set_effect(pdweffect: *mut DROPEFFECT, effect: u32) {
        // SAFETY: OLE 가 준 포인터 (비어 있을 수 있어 확인)
        if let Some(e) = unsafe { pdweffect.as_mut() } {
            *e = DROPEFFECT(effect);
        }
    }

    fn format(cf: u16) -> FORMATETC {
        FORMATETC {
            cfFormat: cf,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn has_text(data: &IDataObject) -> bool {
        // SAFETY: 살아 있는 COM 객체에 지역 변수의 포인터를 넘깁니다.
        unsafe { data.QueryGetData(&format(CF_UNICODETEXT.0)) == S_OK }
    }

    /// 끌어온 파일 경로들 (파일이 아니면 빈 목록)
    fn read_files(data: &IDataObject) -> Vec<PathBuf> {
        let mut files = Vec::new();
        // SAFETY: `GetData` 가 준 메모리는 `ReleaseStgMedium` 까지 유효하고, 버퍼 길이는 먼저 물어본 값입니다.
        unsafe {
            let Ok(mut medium) = data.GetData(&format(CF_HDROP.0)) else { return files };
            if medium.tymed == TYMED_HGLOBAL.0 as u32 {
                let hdrop = HDROP(medium.u.hGlobal.0);
                for i in 0..DragQueryFileW(hdrop, u32::MAX, None) {
                    let len = DragQueryFileW(hdrop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    let written = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
                    files.push(OsString::from_wide(&buf[..written.min(len)]).into());
                }
            }
            ReleaseStgMedium(&mut medium);
        }
        files
    }

    /// 끌어온 글 (없거나 비어 있으면 `None`)
    fn read_text(data: &IDataObject) -> Option<String> {
        let mut text = None;
        // SAFETY: 잠근 메모리는 `GlobalSize` 바이트만큼 읽을 수 있고, 다 읽은 뒤 풀고 돌려줍니다.
        unsafe {
            let mut medium = data.GetData(&format(CF_UNICODETEXT.0)).ok()?;
            if medium.tymed == TYMED_HGLOBAL.0 as u32 {
                let handle = medium.u.hGlobal;
                let ptr = GlobalLock(handle) as *const u16;
                if !ptr.is_null() {
                    text = Some(text_from_utf16(std::slice::from_raw_parts(ptr, GlobalSize(handle) / 2)));
                    let _ = GlobalUnlock(handle);
                }
            }
            ReleaseStgMedium(&mut medium);
        }
        text.filter(|t| !t.is_empty())
    }

    /// 창에 등록된 드롭 타깃(winit 것)을 `ours` 로 바꿉니다.
    /// 등록하지 못하면 원래 것을 되돌려 놓아, 파일 끌어다 놓기는 그대로 되게 합니다.
    pub(super) fn replace(hwnd: HWND, ours: &IDropTarget) -> bool {
        // SAFETY: 창을 만든 UI 스레드(winit 이 OLE 를 초기화해 둠)에서 살아 있는 창에 부릅니다.
        unsafe {
            // OLE 는 등록된 타깃의 포인터를 이 창 속성에 넣어 둡니다 (되돌릴 때만 씀).
            // winit 의 타깃은 창이 닫힐 때까지 winit 이 쥐고 있으므로 포인터는 계속 유효합니다.
            let mut previous = GetPropW(hwnd, w!("OleDropTargetInterface")).0;
            match RevokeDragDrop(hwnd) {
                Ok(()) => {}
                Err(e) if e.code() == DRAGDROP_E_NOTREGISTERED => previous = std::ptr::null_mut(),
                Err(_) => return false,
            }
            if RegisterDragDrop(hwnd, ours).is_ok() {
                return true;
            }
            if let Some(old) = IDropTarget::from_raw_borrowed(&previous) {
                let _ = RegisterDragDrop(hwnd, old);
            }
            false
        }
    }

    pub(super) fn install(cc: &eframe::CreationContext<'_>) {
        let Ok(handle) = cc.window_handle() else { return };
        let RawWindowHandle::Win32(win) = handle.as_raw() else { return };
        let hwnd = HWND(win.hwnd.get() as *mut c_void);
        replace(hwnd, &DropTarget::create(hwnd, cc.egui_ctx.clone(), &super::STATE));
    }

    #[cfg(test)]
    mod tests {
        use std::mem::ManuallyDrop;

        use windows::Win32::Foundation::{DV_E_FORMATETC, E_NOTIMPL, HGLOBAL, OLE_E_ADVISENOTSUPPORTED};
        use windows::Win32::System::Com::{
            IAdviseSink, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, STGMEDIUM, STGMEDIUM_0,
        };
        use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc};
        use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, OleInitialize};
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
        use windows::core::{BOOL, HRESULT};

        use super::super::Zone;
        use super::*;

        /// 끌어온 쪽 프로그램 흉내: 글과 파일 목록을 `HGLOBAL` 로 내줍니다.
        #[implement(IDataObject)]
        struct FakeData {
            text: Option<&'static str>,
            files: Vec<&'static str>,
        }

        impl FakeData {
            fn bytes(&self, cf: u16) -> Option<Vec<u8>> {
                let wide = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
                if cf == CF_UNICODETEXT.0 {
                    let mut out = wide(self.text?);
                    out.extend([0, 0]);
                    Some(out)
                } else if cf == CF_HDROP.0 && !self.files.is_empty() {
                    // DROPFILES: 목록 시작 위치(20), 좌표, fNC, fWide(1) 다음에 0 으로 나뉜 경로들, 끝에 0 하나 더
                    let mut out = Vec::new();
                    out.extend(20u32.to_le_bytes());
                    out.extend([0u8; 12]);
                    out.extend(1u32.to_le_bytes());
                    for f in &self.files {
                        out.extend(wide(f));
                        out.extend([0, 0]);
                    }
                    out.extend([0, 0]);
                    Some(out)
                } else {
                    None
                }
            }
        }

        #[allow(non_snake_case)]
        impl IDataObject_Impl for FakeData_Impl {
            fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
                // SAFETY: 테스트에서 넘긴 유효한 포인터이고, 방금 잡은 메모리에 그 크기만큼만 씁니다.
                unsafe {
                    let bytes = self.bytes((*pformatetcin).cfFormat).ok_or(DV_E_FORMATETC)?;
                    let handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), GlobalLock(handle) as *mut u8, bytes.len());
                    let _ = GlobalUnlock(handle);
                    Ok(STGMEDIUM {
                        tymed: TYMED_HGLOBAL.0 as u32,
                        u: STGMEDIUM_0 { hGlobal: handle },
                        pUnkForRelease: ManuallyDrop::new(None),
                    })
                }
            }

            fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
                Err(E_NOTIMPL.into())
            }

            fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
                // SAFETY: 테스트에서 넘긴 유효한 포인터
                if self.bytes(unsafe { (*pformatetc).cfFormat }).is_some() { S_OK } else { DV_E_FORMATETC }
            }

            fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
                E_NOTIMPL
            }

            fn SetData(&self, _: *const FORMATETC, _: *const STGMEDIUM, _: BOOL) -> Result<()> {
                Err(E_NOTIMPL.into())
            }

            fn EnumFormatEtc(&self, _: u32) -> Result<IEnumFORMATETC> {
                Err(E_NOTIMPL.into())
            }

            fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<IAdviseSink>) -> Result<u32> {
                Err(OLE_E_ADVISENOTSUPPORTED.into())
            }

            fn DUnadvise(&self, _: u32) -> Result<()> {
                Err(OLE_E_ADVISENOTSUPPORTED.into())
            }

            fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
                Err(OLE_E_ADVISENOTSUPPORTED.into())
            }
        }

        const KEYS: MODIFIERKEYS_FLAGS = MODIFIERKEYS_FLAGS(0);

        /// 창 없는 드롭 타깃 (화면 좌표가 그대로 창 안쪽 좌표)과 그 상태. 본문 영역은 (100,100)~(500,400)
        fn target() -> (IDropTarget, &'static Mutex<State>) {
            let state: &'static Mutex<State> = Box::leak(Box::new(Mutex::new(State::new())));
            lock(state).zone = Some(Zone { left: 100, top: 100, right: 500, bottom: 400 });
            (DropTarget::create(HWND::default(), egui::Context::default(), state), state)
        }

        fn data(text: Option<&'static str>, files: &[&'static str]) -> IDataObject {
            FakeData { text, files: files.to_vec() }.into()
        }

        fn all_effects() -> DROPEFFECT {
            DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK
        }

        #[test]
        fn dragged_text_is_followed_and_dropped() {
            let (target, state) = target();
            let data = data(Some("첫 줄\r\n둘째 줄 😀"), &[]);
            // SAFETY: 살아 있는 COM 객체와 지역 변수의 포인터
            unsafe {
                // 사이드바 위로 들어옴: 받지 않는다고 표시하지만 자리는 따라감
                let mut effect = all_effects();
                target.DragEnter(&data, KEYS, POINTL { x: 20, y: 150 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_NONE);
                assert_eq!(lock(state).take().text_at, Some((20, 150)));

                // 본문 위: 복사
                let mut effect = all_effects();
                target.DragOver(KEYS, POINTL { x: 300, y: 200 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_COPY);
                assert_eq!(lock(state).take().text_at, Some((300, 200)));

                let mut effect = all_effects();
                target.Drop(&data, KEYS, POINTL { x: 310, y: 210 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_COPY);
            }
            let pending = lock(state).take();
            assert_eq!(pending.dropped_text, Some(("첫 줄\n둘째 줄 😀".to_owned(), (310, 210))));
            assert_eq!(pending.text_at, None);
            assert!(!pending.text_gone);
            assert_eq!(lock(state).take(), super::super::Pending::default(), "놓은 글은 한 번만 나옴");
        }

        #[test]
        fn text_is_refused_outside_the_editor_or_without_copy() {
            let (target, state) = target();
            let data = data(Some("글"), &[]);
            // SAFETY: 살아 있는 COM 객체와 지역 변수의 포인터
            unsafe {
                // 본문 밖에 놓음
                let mut effect = all_effects();
                target.DragEnter(&data, KEYS, POINTL { x: 20, y: 150 }, &mut effect).unwrap();
                target.Drop(&data, KEYS, POINTL { x: 20, y: 150 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_NONE);
                let pending = lock(state).take();
                assert_eq!((pending.dropped_text, pending.text_at, pending.text_gone), (None, None, true));

                // 이동만 허락하는 쪽에서 끌어옴 (받으면 원래 글이 지워짐)
                let mut effect = DROPEFFECT_MOVE;
                target.DragEnter(&data, KEYS, POINTL { x: 300, y: 200 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_NONE);

                // 대화상자가 떠 있음 (앱이 영역을 비워 둠)
                lock(state).zone = None;
                let mut effect = all_effects();
                target.DragOver(KEYS, POINTL { x: 300, y: 200 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_NONE);

                // 끌다가 창 밖으로
                target.DragLeave().unwrap();
                let pending = lock(state).take();
                assert_eq!((pending.text_at, pending.text_gone), (None, true));
            }
        }

        #[test]
        fn files_are_taken_like_winit_did_with_or_without_text() {
            let paths = [r"C:\메모\할 일.md", r"C:\그림\cat.png"];
            let expected: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            // 파일만 / 파일과 글이 함께 (글은 무시하고 파일로)
            for text in [None, Some(r"C:\메모\할 일.md")] {
                let (target, state) = target();
                let data = data(text, &paths);
                // SAFETY: 살아 있는 COM 객체와 지역 변수의 포인터
                unsafe {
                    // 본문 밖(사이드바)이어도 파일은 받음
                    let mut effect = all_effects();
                    target.DragEnter(&data, KEYS, POINTL { x: 20, y: 150 }, &mut effect).unwrap();
                    assert_eq!(effect.0, EFFECT_COPY);
                    let pending = lock(state).take();
                    assert_eq!(pending.hovered_files, expected);
                    assert_eq!(pending.text_at, None);

                    let mut effect = all_effects();
                    target.DragOver(KEYS, POINTL { x: 30, y: 150 }, &mut effect).unwrap();
                    assert_eq!(effect.0, EFFECT_COPY);
                    assert_eq!(lock(state).take().hovered_files, expected, "끄는 동안 계속 보임");

                    let mut effect = all_effects();
                    target.Drop(&data, KEYS, POINTL { x: 30, y: 150 }, &mut effect).unwrap();
                    assert_eq!(effect.0, EFFECT_COPY);
                }
                let pending = lock(state).take();
                assert_eq!(pending.dropped_files, expected);
                assert!(pending.hovered_files.is_empty());
                assert_eq!((pending.dropped_text, pending.text_at, pending.text_gone), (None, None, false));
                assert_eq!(lock(state).take(), super::super::Pending::default(), "놓은 파일은 한 번만 나옴");
            }
        }

        #[test]
        fn leaving_clears_hovered_files_and_other_data_is_refused() {
            let (target, state) = target();
            // SAFETY: 살아 있는 COM 객체와 지역 변수의 포인터
            unsafe {
                let mut effect = all_effects();
                target.DragEnter(&data(None, &[r"C:\a.md"]), KEYS, POINTL { x: 300, y: 200 }, &mut effect).unwrap();
                target.DragLeave().unwrap();
                assert_eq!(lock(state).take(), super::super::Pending::default());

                // 글도 파일도 아님
                let nothing = data(None, &[]);
                let mut effect = all_effects();
                target.DragEnter(&nothing, KEYS, POINTL { x: 300, y: 200 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_NONE);
                let mut effect = all_effects();
                target.Drop(&nothing, KEYS, POINTL { x: 300, y: 200 }, &mut effect).unwrap();
                assert_eq!(effect.0, EFFECT_NONE);
                assert_eq!(lock(state).take(), super::super::Pending::default());
            }
        }

        /// 이미 다른 드롭 타깃(앱에서는 winit 것)이 등록된 창에서 우리 것으로 바꿉니다.
        /// 되돌릴 때 쓰는 창 속성이 정말 등록된 타깃의 포인터인지도 여기서 확인합니다.
        #[test]
        fn replaces_the_registered_drop_target() {
            // SAFETY: 이 스레드에서 만든 보이지 않는 창에만 부릅니다 (다른 창이나 입력은 건드리지 않음).
            unsafe {
                OleInitialize(None).unwrap();
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("drop target test"),
                    WS_POPUP,
                    0,
                    0,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                let registered = || GetPropW(hwnd, w!("OleDropTargetInterface")).0;

                let (first, _) = target();
                RegisterDragDrop(hwnd, &first).unwrap();
                assert_eq!(registered(), first.as_raw());

                let (ours, _) = target();
                assert!(replace(hwnd, &ours));
                assert_eq!(registered(), ours.as_raw());

                // 등록된 것이 없던 창에도 등록
                RevokeDragDrop(hwnd).unwrap();
                assert!(replace(hwnd, &ours));
                assert_eq!(registered(), ours.as_raw());

                RevokeDragDrop(hwnd).unwrap();
                DestroyWindow(hwnd).unwrap();
            }
        }
    }
}

/// 창이 만들어진 직후 한 번 호출합니다.
pub fn install(cc: &eframe::CreationContext<'_>) {
    #[cfg(windows)]
    imp::install(cc);
    #[cfg(not(windows))]
    let _ = cc;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_preferred_over_text() {
        assert_eq!(choose_kind(true, true), Some(Kind::Files));
        assert_eq!(choose_kind(true, false), Some(Kind::Files));
        assert_eq!(choose_kind(false, true), Some(Kind::Text));
        assert_eq!(choose_kind(false, false), None);
    }

    #[test]
    fn text_ends_at_nul_and_uses_lf() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(text_from_utf16(&units("한 줄\r\n두 줄\r셋째\n끝\0쓰레기\0")), "한 줄\n두 줄\n셋째\n끝");
        assert_eq!(text_from_utf16(&units("끝 표시 없음 😀")), "끝 표시 없음 😀");
        assert_eq!(text_from_utf16(&[]), "");
    }

    #[test]
    fn client_pixels_become_points() {
        // 창 배율 150%: 픽셀 300 은 200pt
        assert_eq!(client_to_points((300, 150), 1.5), egui::pos2(200.0, 100.0));
        // 창 배율 125% × 화면 확대 120% = 1.5
        assert_eq!(client_to_points((600, 45), 1.25 * 1.2), egui::pos2(400.0, 30.0));
        assert_eq!(client_to_points((-10, 7), 1.0), egui::pos2(-10.0, 7.0));
    }

    #[test]
    fn zone_is_the_editor_area_in_pixels() {
        let rect = egui::Rect::from_min_max(egui::pos2(250.0, 44.0), egui::pos2(1000.0, 700.0));
        let zone = Zone::from_points(rect, 1.5);
        assert_eq!(zone, Zone { left: 375, top: 66, right: 1500, bottom: 1050 });
        assert!(zone.contains((375, 66)));
        assert!(zone.contains((1499, 1049)));
        assert!(!zone.contains((374, 500)), "사이드바");
        assert!(!zone.contains((800, 65)), "위쪽 막대");
        assert!(!zone.contains((1500, 500)));
        // 왕복: 영역 안의 픽셀은 egui 좌표로도 그 사각형 안
        assert!(rect.contains(client_to_points((376, 67), 1.5)));
    }

    #[test]
    fn text_is_copied_only_over_the_editor() {
        assert_eq!(text_effect(EFFECT_COPY | 2 | 4, true), EFFECT_COPY);
        assert_eq!(text_effect(EFFECT_COPY | 2 | 4, false), EFFECT_NONE);
        assert_eq!(text_effect(2, true), EFFECT_NONE, "이동만 허락하면 받지 않음");
    }

    #[test]
    fn files_go_to_egui_like_winit_did() {
        let mut raw = egui::RawInput::default();
        let pending = Pending { hovered_files: vec![PathBuf::from("a.md")], ..Default::default() };
        assert_eq!(feed(pending, 1.0, &mut raw), TextDrag::default());
        assert_eq!(raw.hovered_files.len(), 1);
        assert_eq!(raw.hovered_files[0].path.as_deref(), Some(Path::new("a.md")));
        assert!(raw.dropped_files.is_empty() && raw.events.is_empty());

        let mut raw = egui::RawInput::default();
        let pending =
            Pending { dropped_files: vec![PathBuf::from("a.md"), PathBuf::from("b.png")], ..Default::default() };
        assert_eq!(feed(pending, 1.0, &mut raw), TextDrag::default());
        let paths: Vec<&Path> = raw.dropped_files.iter().map(|f| f.path()).collect();
        assert_eq!(paths, [Path::new("a.md"), Path::new("b.png")]);
        assert!(raw.hovered_files.is_empty() && raw.events.is_empty());
    }

    #[test]
    fn dragged_text_moves_the_pointer() {
        let mut raw = egui::RawInput::default();
        let drag = feed(Pending { text_at: Some((300, 150)), ..Default::default() }, 1.5, &mut raw);
        let pos = egui::pos2(200.0, 100.0);
        assert_eq!(drag, TextDrag { hover: Some(pos), dropped: None });
        assert_eq!(raw.events, [egui::Event::PointerMoved(pos)]);

        let mut raw = egui::RawInput::default();
        let pending = Pending { dropped_text: Some(("글".into(), (300, 150))), ..Default::default() };
        let drag = feed(pending, 1.5, &mut raw);
        assert_eq!(drag, TextDrag { hover: None, dropped: Some(("글".into(), pos)) });
        assert_eq!(raw.events, [egui::Event::PointerMoved(pos)]);

        // 창 밖으로 나가면 포인터도 사라짐. 나갔다가 바로 다시 들어왔으면 그대로 따라감
        let mut raw = egui::RawInput::default();
        assert_eq!(feed(Pending { text_gone: true, ..Default::default() }, 1.5, &mut raw), TextDrag::default());
        assert_eq!(raw.events, [egui::Event::PointerGone]);
        let mut raw = egui::RawInput::default();
        feed(Pending { text_gone: true, text_at: Some((3, 3)), ..Default::default() }, 1.0, &mut raw);
        assert_eq!(raw.events, [egui::Event::PointerMoved(egui::pos2(3.0, 3.0))]);

        let mut raw = egui::RawInput::default();
        assert_eq!(feed(Pending::default(), 1.0, &mut raw), TextDrag::default());
        assert!(raw.events.is_empty());
    }
}
