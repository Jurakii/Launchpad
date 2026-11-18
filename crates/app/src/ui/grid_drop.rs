//! Files dragged in from Explorer. Slint doesn't hand OS drops over, so the
//! window gets our own OLE `IDropTarget` in place of winit's.
use std::cell::Cell;
use windows::core::{implement, Ref, Result};
use windows::Win32::Foundation::{HWND, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_NONE, MK_ALT,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// What a drop reports: the paths, whether Alt was held, and the point in
/// the window's client area (physical px).
pub type OnDrop = fn(Vec<String>, bool, i32, i32);
/// Called with true when files come over the window, false when they leave.
pub type OnHover = fn(bool);

#[implement(IDropTarget)]
struct Target {
    hwnd: HWND,
    files: Cell<bool>,
    on_drop: OnDrop,
    on_hover: OnHover,
}

fn format() -> FORMATETC {
    FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 }
}

fn paths(data: &IDataObject) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let Ok(mut medium) = data.GetData(&format()) else { return out };
        let drop = HDROP(medium.u.hGlobal.0);
        for i in 0..DragQueryFileW(drop, u32::MAX, None) {
            let mut buf = vec![0u16; DragQueryFileW(drop, i, None) as usize + 1];
            let n = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            out.push(String::from_utf16_lossy(&buf[..n]));
        }
        ReleaseStgMedium(&mut medium);
    }
    out
}

impl Target_Impl {
    fn effect(&self, keys: MODIFIERKEYS_FLAGS, out: *mut DROPEFFECT) {
        let e = match (self.files.get(), keys.0 & MK_ALT != 0) {
            (false, _) => DROPEFFECT_NONE,
            (true, true) => DROPEFFECT_LINK,
            (true, false) => DROPEFFECT_COPY,
        };
        if !out.is_null() {
            unsafe { *out = e };
        }
    }
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let has = data.as_ref().is_some_and(|d| unsafe { d.QueryGetData(&format()) }.is_ok());
        self.files.set(has);
        self.effect(keys, effect);
        if has {
            (self.on_hover)(true);
        }
        Ok(())
    }

    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        self.effect(keys, effect);
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        if self.files.replace(false) {
            (self.on_hover)(false);
        }
        Ok(())
    }

    fn Drop(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        self.effect(keys, effect);
        if !self.files.replace(false) {
            return Ok(());
        }
        (self.on_hover)(false);
        let list = data.as_ref().map(paths).unwrap_or_default();
        let mut p = POINT { x: pt.x, y: pt.y };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut p);
        }
        if !list.is_empty() {
            (self.on_drop)(list, keys.0 & MK_ALT != 0, p.x, p.y);
        }
        Ok(())
    }
}

/// Puts our drop target on `hwnd`, replacing winit's. The registration goes
/// with the window (OLE drops it when the HWND is destroyed).
pub fn register(hwnd: HWND, on_drop: OnDrop, on_hover: OnHover) -> Result<()> {
    let target: IDropTarget = Target { hwnd, files: Cell::new(false), on_drop, on_hover }.into();
    unsafe {
        let _ = RevokeDragDrop(hwnd);
        if RegisterDragDrop(hwnd, &target).is_err() {
            // winit only initialises OLE when its own drag and drop is on.
            OleInitialize(None)?;
            RegisterDragDrop(hwnd, &target)?;
        }
    }
    Ok(())
}
