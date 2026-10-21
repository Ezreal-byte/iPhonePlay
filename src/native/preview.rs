//! The preview child is hit-test transparent and may have display-only corners.
//! Mouse input reaches the egui parent without stealing keyboard focus. This is
//! WM_NCHITTEST routing, not WS_EX_TRANSPARENT painting/compositing behavior.
use std::{marker::PhantomData, rc::Rc, sync::OnceLock};
use windows::{
    Win32::{
        Foundation::{
            ERROR_SUCCESS, GetLastError, HWND, LPARAM, LRESULT, RECT, SetLastError, WPARAM,
        },
        Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

static CLASS_REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
unsafe extern "system" fn preview_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

pub struct PreviewWindow {
    hwnd: HWND,
    rect: Option<(i32, i32, i32, i32)>,
    visible: bool,
    corner_radius: u32,
    // Creation, positioning and destruction must stay on the UI thread.
    _ui_thread: PhantomData<Rc<()>>,
}
impl PreviewWindow {
    pub fn new(parent: isize) -> Result<Self, String> {
        if parent == 0 {
            return Err("主窗口句柄尚未建立".into());
        }
        CLASS_REGISTERED
            .get_or_init(|| unsafe {
                let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(preview_proc),
                    hInstance: instance.into(),
                    lpszClassName: w!("iPhonePlay.NativePreview.v1"),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    Err(format!(
                        "无法注册原生预览窗口：{}",
                        windows::core::Error::from_thread()
                    ))
                } else {
                    Ok(())
                }
            })
            .clone()?;
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("iPhonePlay.NativePreview.v1"),
                w!(""),
                WS_CHILD | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(HWND(parent as *mut _)),
                None,
                Some(instance.into()),
                None,
            )
        }
        .map_err(|e| format!("无法创建原生预览窗口：{e}"))?;
        Ok(Self {
            hwnd,
            rect: None,
            visible: false,
            corner_radius: 0,
            _ui_thread: PhantomData,
        })
    }
    pub fn hwnd(&self) -> usize {
        self.hwnd.0 as usize
    }
    /// Coordinates are parent-client physical pixels, after egui scale and DPI.
    pub fn set_rect(&mut self, x: i32, y: i32, width: i32, height: i32) -> Result<(), String> {
        let rect = (x, y, width.max(1), height.max(1));
        if self.rect == Some(rect) {
            return Ok(());
        }
        unsafe {
            SetWindowPos(
                self.hwnd,
                None,
                rect.0,
                rect.1,
                rect.2,
                rect.3,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
        }
        .map_err(|e| format!("无法调整预览窗口：{e}"))?;
        self.apply_corner_region()?;
        self.rect = Some(rect);
        Ok(())
    }
    /// Move the same rendering HWND between hosts without replacing its session.
    /// The child stays hidden until the caller lays it out and shows it again.
    pub fn reparent(&mut self, parent: isize) -> Result<(), String> {
        let parent = HWND(parent as *mut _);
        if parent.0.is_null() || !unsafe { IsWindow(Some(parent)) }.as_bool() {
            return Err("投屏目标窗口已经关闭".into());
        }
        if self.hwnd.0.is_null() {
            return Err("投屏预览窗口已经关闭".into());
        }
        if unsafe { GetParent(self.hwnd) }.ok() == Some(parent) {
            return Ok(());
        }
        let was_visible = self.visible;
        self.set_visible(false);
        // SetParent may return NULL on success when the old parent was NULL.
        // windows-rs maps that return value to Err, so also check LastError.
        unsafe {
            SetLastError(ERROR_SUCCESS);
        }
        let result = unsafe { SetParent(self.hwnd, Some(parent)) };
        if let Err(error) = result
            && unsafe { GetLastError() } != ERROR_SUCCESS
        {
            self.set_visible(was_visible);
            return Err(format!("无法移动投屏预览窗口：{error}"));
        }
        // Parent-client coordinates and DPI have changed. Force the next layout.
        self.rect = None;
        Ok(())
    }
    /// Physical pixels. This changes only HWND clipping, never decoded frames.
    /// Zero removes clipping. Windows owns each successfully installed region.
    pub fn set_corner_radius(&mut self, radius: u32) -> Result<(), String> {
        if self.corner_radius == radius {
            return Ok(());
        }
        let previous = self.corner_radius;
        self.corner_radius = radius;
        if let Err(error) = self.apply_corner_region() {
            self.corner_radius = previous;
            return Err(error);
        }
        Ok(())
    }
    fn apply_corner_region(&self) -> Result<(), String> {
        let mut rect = RECT::default();
        unsafe { GetClientRect(self.hwnd, &mut rect) }
            .map_err(|e| format!("无法读取预览尺寸：{e}"))?;
        let diameter = corner_diameter(rect.right, rect.bottom, self.corner_radius);
        let region = if diameter > 0 {
            // GDI's rounded-region rasterization excludes the trailing boundary.
            // +1 keeps the final content row/column instead of clipping a pixel.
            let region = unsafe {
                CreateRoundRectRgn(
                    0,
                    0,
                    rect.right.saturating_add(1),
                    rect.bottom.saturating_add(1),
                    diameter,
                    diameter,
                )
            };
            if region.0.is_null() {
                return Err("无法创建预览圆角区域".into());
            }
            Some(region)
        } else {
            None
        };
        if unsafe { SetWindowRgn(self.hwnd, region, true) } == 0 {
            let error = windows::core::Error::from_thread();
            // On failure ownership stays with us; on success the OS deletes it.
            if let Some(region) = region {
                unsafe {
                    let _ = DeleteObject(region.into());
                }
            }
            return Err(format!("无法设置预览圆角：{error}"));
        }
        Ok(())
    }
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible && !self.hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
            self.visible = visible;
        }
    }
    /// Detach native rendering through NativeBackend before explicit destroy.
    pub fn destroy(&mut self) {
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            self.hwnd = HWND::default();
            self.visible = false;
            self.rect = None;
        }
    }
}

fn corner_diameter(width: i32, height: i32, radius: u32) -> i32 {
    radius
        .saturating_mul(2)
        .min(width.max(0) as u32)
        .min(height.max(0) as u32) as i32
}

impl Drop for PreviewWindow {
    fn drop(&mut self) {
        self.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::corner_diameter;

    #[test]
    fn rounded_region_is_bounded_and_zero_removes_clipping() {
        assert_eq!(corner_diameter(300, 600, 18), 36);
        assert_eq!(corner_diameter(300, 600, 0), 0);
        assert_eq!(corner_diameter(11, 600, 18), 11);
        assert_eq!(corner_diameter(300, 600, u32::MAX), 300);
        assert_eq!(corner_diameter(-1, 600, 18), 0);
        assert_eq!(corner_diameter(300, 0, 18), 0);
    }
}
