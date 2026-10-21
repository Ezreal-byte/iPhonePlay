//! A UI-thread native host for the existing preview child HWND.
//!
//! WM_CLOSE is a request: the app reparents its preview to the main window, then
//! drops this host. No message handler touches the capture or recorder backends.
use std::{cell::Cell, marker::PhantomData, rc::Rc, sync::OnceLock};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BLACK_BRUSH, GetMonitorInfoW, GetStockObject, HBRUSH, MONITOR_DEFAULTTONEAREST,
            MONITORINFO, MonitorFromWindow,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

static CLASS_REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_CLIPCHILDREN.0);
const EX_STYLE: WINDOW_EX_STYLE = WINDOW_EX_STYLE(WS_EX_APPWINDOW.0 | WS_EX_TOPMOST.0);

#[derive(Default)]
struct WindowState {
    close_requested: Cell<bool>,
    video_clicked: Cell<bool>,
    layout_changed: Cell<bool>,
    minimized: Cell<bool>,
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        return LRESULT(1);
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const WindowState;
    let state = unsafe { pointer.as_ref() };
    match message {
        WM_CLOSE => {
            if let Some(state) = state {
                state.close_requested.set(true);
            }
            // DefWindowProc would destroy the host AND its live preview child.
            return LRESULT(0);
        }
        WM_SIZE => {
            if let Some(state) = state {
                state.minimized.set(wparam.0 == SIZE_MINIMIZED as usize);
                state.layout_changed.set(true);
            }
        }
        WM_LBUTTONDOWN => {
            if let Some(state) = state {
                state.video_clicked.set(true);
            }
        }
        WM_DPICHANGED => {
            if lparam.0 != 0 {
                let suggested = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        suggested.left,
                        suggested.top,
                        suggested.right - suggested.left,
                        suggested.bottom - suggested.top,
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                }
            }
            if let Some(state) = state {
                state.layout_changed.set(true);
            }
            return LRESULT(0);
        }
        WM_GETMINMAXINFO => {
            if lparam.0 != 0 {
                let limits = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                let dpi = window_dpi(hwnd);
                limits.ptMinTrackSize.x = logical_pixels(160, dpi);
                limits.ptMinTrackSize.y = logical_pixels(160, dpi);
            }
            return LRESULT(0);
        }
        WM_NCDESTROY => unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        },
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

pub struct FloatingWindow {
    hwnd: HWND,
    main: HWND,
    // Stable allocation for GWLP_USERDATA; released after DestroyWindow returns.
    state: Box<WindowState>,
    topmost: bool,
    _ui_thread: PhantomData<Rc<()>>,
}

impl FloatingWindow {
    /// Creates an independent top-level window on the main window's monitor.
    /// It remains visible if the main window is minimized; topmost defaults on.
    pub fn new(parent: isize, title: &str, source: [u32; 2]) -> Result<Self, String> {
        Self::create(parent, title, source, true)
    }

    fn create(parent: isize, title: &str, source: [u32; 2], visible: bool) -> Result<Self, String> {
        let main = HWND(parent as *mut _);
        if main.0.is_null() || !unsafe { IsWindow(Some(main)) }.as_bool() {
            return Err("主窗口句柄无效，无法打开悬浮投屏".into());
        }
        let title = wide_title(title)?;
        CLASS_REGISTERED
            .get_or_init(|| unsafe {
                let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance.into(),
                    hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
                    hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                    lpszClassName: w!("iPhonePlay.FloatingVideo.v1"),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    Err(format!(
                        "无法注册悬浮窗口：{}",
                        windows::core::Error::from_thread()
                    ))
                } else {
                    Ok(())
                }
            })
            .clone()?;

        let dpi = window_dpi(main);
        let mut monitor = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe {
            GetMonitorInfoW(
                MonitorFromWindow(main, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            )
        }
        .as_bool()
        {
            return Err("无法读取悬浮窗口所在显示器的工作区".into());
        }
        let work = monitor.rcWork;
        let mut frame = RECT::default();
        unsafe { AdjustWindowRectExForDpi(&mut frame, STYLE, false, EX_STYLE, dpi) }
            .map_err(|e| format!("无法计算悬浮窗口边框：{e}"))?;
        let frame_width = frame.right - frame.left;
        let frame_height = frame.bottom - frame.top;
        let available = [
            ((work.right - work.left - frame_width).max(1) as u32 * 9 / 10).max(1),
            ((work.bottom - work.top - frame_height).max(1) as u32 * 9 / 10).max(1),
        ];
        let bounds = [
            available[0].min(logical_pixels(600, dpi) as u32),
            available[1].min(logical_pixels(720, dpi) as u32),
        ];
        let source = if source.contains(&0) { [9, 16] } else { source };
        let video = fit_video_rect(bounds, source);
        let width = (video[2] + frame_width).max(1);
        let height = (video[3] + frame_height).max(1);
        let x = work.left + ((work.right - work.left - width) / 2).max(0);
        let y = work.top + ((work.bottom - work.top - height) / 2).max(0);
        let state = Box::<WindowState>::default();
        let hwnd = unsafe {
            CreateWindowExW(
                EX_STYLE,
                w!("iPhonePlay.FloatingVideo.v1"),
                PCWSTR(title.as_ptr()),
                STYLE,
                x,
                y,
                width,
                height,
                None,
                None,
                Some(GetModuleHandleW(None).map_err(|e| e.to_string())?.into()),
                Some((&*state as *const WindowState).cast()),
            )
        }
        .map_err(|e| format!("无法创建悬浮投屏窗口：{e}"))?;
        unsafe {
            for kind in [ICON_SMALL, ICON_BIG] {
                let icon = SendMessageW(
                    main,
                    WM_GETICON,
                    Some(WPARAM(kind as usize)),
                    Some(LPARAM(0)),
                );
                if icon.0 != 0 {
                    SendMessageW(
                        hwnd,
                        WM_SETICON,
                        Some(WPARAM(kind as usize)),
                        Some(LPARAM(icon.0)),
                    );
                }
            }
            if visible {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
        Ok(Self {
            hwnd,
            main,
            state,
            topmost: true,
            _ui_thread: PhantomData,
        })
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Physical client pixels, including any letterbox area; zero if minimized.
    pub fn client_size(&self) -> [u32; 2] {
        if self.is_minimized() {
            return [0, 0];
        }
        let mut rect = RECT::default();
        if unsafe { GetClientRect(self.hwnd, &mut rect) }.is_err() {
            return [0, 0];
        }
        [
            (rect.right - rect.left).max(0) as u32,
            (rect.bottom - rect.top).max(0) as u32,
        ]
    }

    pub fn dpi(&self) -> u32 {
        window_dpi(self.hwnd)
    }
    pub fn is_minimized(&self) -> bool {
        self.state.minimized.get()
    }
    pub fn take_close_requested(&self) -> bool {
        self.state.close_requested.replace(false)
    }
    pub fn take_video_clicked(&self) -> bool {
        self.state.video_clicked.replace(false)
    }
    pub fn take_layout_changed(&self) -> bool {
        self.state.layout_changed.replace(false)
    }

    pub fn set_topmost(&mut self, enabled: bool) -> Result<(), String> {
        if self.topmost != enabled {
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    Some(if enabled {
                        HWND_TOPMOST
                    } else {
                        HWND_NOTOPMOST
                    }),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
            }
            .map_err(|e| format!("无法调整悬浮窗口置顶状态：{e}"))?;
            self.topmost = enabled;
        }
        Ok(())
    }

    pub fn set_title(&self, title: &str) -> Result<(), String> {
        let title = wide_title(title)?;
        unsafe { SetWindowTextW(self.hwnd, PCWSTR(title.as_ptr())) }
            .map_err(|e| format!("无法更新悬浮窗口标题：{e}"))
    }
}

impl Drop for FloatingWindow {
    fn drop(&mut self) {
        // Normal path: the app has already reparented its PreviewWindow. This
        // fallback prevents accidental child destruction if that step was missed.
        while let Ok(child) = unsafe { GetWindow(self.hwnd, GW_CHILD) } {
            unsafe {
                let _ = ShowWindow(child, SW_HIDE);
            }
            if unsafe { SetParent(child, Some(self.main)) }.is_err() {
                // Never invalidate a live renderer's HWND on an error. Leave the
                // hidden host alive, with no userdata pointing at freed memory.
                unsafe {
                    SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
                tracing::error!(
                    "Cannot return preview child; preserving native HWND until process exit"
                );
                return;
            }
        }
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn window_dpi(hwnd: HWND) -> u32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 96,
        dpi => dpi,
    }
}

fn logical_pixels(pixels: u32, dpi: u32) -> i32 {
    ((u64::from(pixels) * u64::from(dpi) + 48) / 96).min(i32::MAX as u64) as i32
}

fn wide_title(title: &str) -> Result<Vec<u16>, String> {
    if title.contains('\0') {
        return Err("窗口标题不能包含空字符".into());
    }
    Ok(title.encode_utf16().chain(std::iter::once(0)).collect())
}

/// Fit an unmodified source frame into physical client pixels with equal margins.
/// Returns [x, y, width, height]; invalid or minimized dimensions yield no video.
pub fn fit_video_rect(client: [u32; 2], source: [u32; 2]) -> [i32; 4] {
    if client.contains(&0) || source.contains(&0) {
        return [0; 4];
    }
    let [cw, ch] = client.map(|v| u64::from(v.min(i32::MAX as u32)));
    let [sw, sh] = source.map(u64::from);
    let (width, height) = if cw * sh > ch * sw {
        ((ch * sw / sh).max(1), ch)
    } else {
        (cw, (cw * sh / sw).max(1))
    };
    [
        ((cw - width) / 2) as i32,
        ((ch - height) / 2) as i32,
        width as i32,
        height as i32,
    ]
}

#[cfg(test)]
mod tests {
    use super::{fit_video_rect, logical_pixels, wide_title};

    #[test]
    fn video_fit_keeps_portrait_and_landscape_content_inside_client() {
        assert_eq!(fit_video_rect([600, 600], [1080, 1920]), [131, 0, 337, 600]);
        assert_eq!(fit_video_rect([600, 600], [1920, 1080]), [0, 131, 600, 337]);
        assert_eq!(fit_video_rect([600, 400], [1200, 800]), [0, 0, 600, 400]);
        assert_eq!(fit_video_rect([0, 600], [1080, 1920]), [0; 4]);
        assert_eq!(fit_video_rect([600, 600], [0, 1920]), [0; 4]);
        for source in [[1, u32::MAX], [u32::MAX, 1], [u32::MAX; 2]] {
            let [x, y, width, height] = fit_video_rect([800, 600], source);
            assert!(width > 0 && height > 0 && x >= 0 && y >= 0);
            assert!(x + width <= 800 && y + height <= 600);
        }
    }

    #[test]
    fn dpi_and_unicode_title_do_not_truncate_or_overflow() {
        assert_eq!(logical_pixels(160, 96), 160);
        assert_eq!(logical_pixels(160, 120), 200);
        assert_eq!(logical_pixels(160, 192), 320);
        assert_eq!(logical_pixels(u32::MAX, u32::MAX), i32::MAX);
        let title = wide_title("iPhonePlay · 投屏 📱").unwrap();
        assert_eq!(title.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&title[..title.len() - 1]).unwrap(),
            "iPhonePlay · 投屏 📱"
        );
        assert!(wide_title("title\0tail").is_err());
    }

    #[test]
    #[ignore = "Creates hidden native HWNDs; run explicitly on Windows"]
    fn hidden_preview_reparent_rounding_lifecycle_smoke() {
        use super::{FloatingWindow, HWND};
        use iphoneplay::native::PreviewWindow;
        use windows::{
            Win32::{Foundation::RECT, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
            core::w,
        };

        struct HiddenParent(HWND);
        impl Drop for HiddenParent {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let main = HiddenParent(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("iPhonePlay hidden lifecycle test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                800,
                600,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        });
        let foreground = unsafe { GetForegroundWindow() };
        let mut floating =
            FloatingWindow::create(main.0.0 as isize, "隐藏窗口测试", [1080, 1920], false).unwrap();
        let host = HWND(floating.hwnd() as *mut _);
        assert!(!unsafe { IsWindowVisible(host) }.as_bool());
        assert!(
            unsafe { GetWindow(host, GW_OWNER) }.is_err(),
            "host must be independent of main-window minimizing"
        );
        for enabled in [true, true, false, false, true] {
            floating.set_topmost(enabled).unwrap();
            let style = unsafe { GetWindowLongPtrW(host, GWL_EXSTYLE) } as u32;
            assert_eq!(style & WS_EX_TOPMOST.0 != 0, enabled);
            assert_eq!(unsafe { GetForegroundWindow() }, foreground);
            assert!(!unsafe { IsWindowVisible(host) }.as_bool());
        }
        let mut preview = PreviewWindow::new(main.0.0 as isize).unwrap();
        let child = HWND(preview.hwnd() as *mut _);
        preview.reparent(floating.hwnd()).unwrap();
        assert_eq!(unsafe { GetParent(child) }.unwrap(), host);
        preview.set_rect(10, 20, 270, 480).unwrap();
        preview.set_corner_radius(18).unwrap();
        let region = unsafe { CreateRectRgn(0, 0, 0, 0) };
        assert!(!region.0.is_null());
        assert_ne!(unsafe { GetWindowRgn(child, region) }, GDI_REGION_TYPE(0));
        assert!(!unsafe { PtInRegion(region, 0, 0) }.as_bool());
        assert!(unsafe { PtInRegion(region, 135, 240) }.as_bool());
        let mut bounds = RECT::default();
        unsafe {
            GetRgnBox(region, &mut bounds);
        }
        assert_eq!((bounds.right, bounds.bottom), (270, 480));
        preview.set_rect(0, 0, 480, 270).unwrap();
        unsafe {
            GetWindowRgn(child, region);
            GetRgnBox(region, &mut bounds);
        }
        assert_eq!((bounds.right, bounds.bottom), (480, 270));
        preview.set_corner_radius(0).unwrap();
        assert_eq!(unsafe { GetWindowRgn(child, region) }, GDI_REGION_TYPE(0));
        unsafe {
            assert!(DeleteObject(region.into()).as_bool());
        }
        unsafe {
            SendMessageW(host, WM_CLOSE, None, None);
        }
        assert!(floating.take_close_requested());
        assert!(!floating.take_close_requested());
        assert!(unsafe { IsWindow(Some(host)) }.as_bool());
        assert!(unsafe { IsWindow(Some(child)) }.as_bool());
        preview.reparent(main.0.0 as isize).unwrap();
        drop(floating);
        assert!(!unsafe { IsWindow(Some(host)) }.as_bool());
        assert!(unsafe { IsWindow(Some(child)) }.as_bool());
        assert_eq!(unsafe { GetParent(child) }.unwrap(), main.0);

        // Exercise NULL previous-parent success through windows-rs's Err mapping.
        unsafe {
            SetParent(child, None).unwrap();
            // WS_CHILD at the desktop still reports the desktop as its parent.
            // Convert the hidden test child to a top-level HWND to cover NULL.
            SetWindowLongPtrW(child, GWL_STYLE, WS_OVERLAPPEDWINDOW.0 as isize);
        }
        assert!(unsafe { GetParent(child) }.is_err());
        preview.reparent(main.0.0 as isize).unwrap();
        unsafe {
            SetWindowLongPtrW(child, GWL_STYLE, (WS_CHILD | WS_CLIPSIBLINGS).0 as isize);
        }
        assert_eq!(unsafe { GetParent(child) }.unwrap(), main.0);

        let fallback =
            FloatingWindow::create(main.0.0 as isize, "隐藏窗口清理测试", [1920, 1080], false)
                .unwrap();
        let fallback_hwnd = HWND(fallback.hwnd() as *mut _);
        preview.reparent(fallback.hwnd()).unwrap();
        drop(fallback);
        assert!(!unsafe { IsWindow(Some(fallback_hwnd)) }.as_bool());
        assert!(unsafe { IsWindow(Some(child)) }.as_bool());
        assert_eq!(unsafe { GetParent(child) }.unwrap(), main.0);
        preview.destroy();
        assert!(!unsafe { IsWindow(Some(child)) }.as_bool());
        assert_eq!(unsafe { GetForegroundWindow() }, foreground);
    }
}
