use eframe::egui;
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows::{
    Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        UI::Shell::ShellExecuteW,
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNORMAL,
        },
    },
    core::{PCWSTR, w},
};

pub struct Instance(windows::Win32::Foundation::HANDLE);
impl Instance {
    pub fn acquire() -> Result<Option<Self>, String> {
        use windows::Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::CreateMutexW,
            UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow},
        };
        let handle =
            unsafe { CreateMutexW(None, false, w!("Local\\iPhonePlay.Desktop.SingleInstance")) }
                .map_err(|e| e.to_string())?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
                if let Ok(hwnd) = FindWindowW(None, w!("iPhonePlay")) {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            Ok(None)
        } else {
            Ok(Some(Self(handle)))
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[derive(Debug)]
pub enum BackgroundEvent {
    Directory { screenshot: bool, path: PathBuf },
    Exported(PathBuf),
    Failed(String),
    UiCaptured,
}

pub struct Tray {
    _icon: TrayIcon,
    show: MenuItem,
    quit: MenuItem,
}
impl Tray {
    pub fn new(ctx: egui::Context) -> Result<Self, String> {
        let menu = Menu::new();
        let show = MenuItem::new("打开 iPhonePlay", true, None);
        let quit = MenuItem::new("退出 iPhonePlay", true, None);
        menu.append_items(&[&show, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| e.to_string())?;
        let icon = app_icon();
        let tray_icon = tray_icon::Icon::from_rgba(icon.rgba, icon.width, icon.height)
            .map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("iPhonePlay · 投屏和录制在后台继续")
            .with_icon(tray_icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        let wake = ctx.clone();
        // Keep tray delivery on the UI thread; eframe also polls while hidden.
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                wake.request_repaint();
            }
        });
        Ok(Self {
            _icon: icon,
            show,
            quit,
        })
    }
    pub fn poll(&self) -> Option<bool> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.quit.id() {
                return Some(false);
            }
            if event.id == self.show.id() {
                return Some(true);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                return Some(true);
            }
        }
        None
    }
}

pub fn runtime_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("IPHONEPLAY_RUNTIME") {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let base = exe.parent().unwrap_or(Path::new("."));
    if base.join("iPhoneMirror.Core.dll").is_file() {
        return base.to_path_buf();
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime");
    if dev.is_dir() {
        return dev;
    }
    base.to_path_buf()
}
pub fn ffmpeg(runtime: &Path) -> PathBuf {
    for name in [
        "tools/ffmpeg/ffmpeg.exe",
        "ffmpeg/ffmpeg.exe",
        "ffmpeg.exe",
        "tools/ffmpeg.exe",
    ] {
        let p = runtime.join(name);
        if p.is_file() {
            return p;
        }
    }
    runtime.join("tools/ffmpeg/ffmpeg.exe")
}
pub fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/app.png")).ok();
    if let Some(image) = img {
        let image = image
            .resize(64, 64, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        return egui::IconData {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        };
    }
    // Resource fallback for a build before the generated app asset was installed.
    egui::IconData {
        width: 16,
        height: 16,
        rgba: [49, 148, 244, 255].repeat(16 * 16),
    }
}
pub fn foreground(hwnd: isize) -> bool {
    unsafe { GetForegroundWindow() == HWND(hwnd as *mut _) }
}
pub fn screen_rect(hwnd: isize, rect: egui::Rect, scale: f32) -> [i32; 4] {
    let mut p = POINT {
        x: (rect.min.x * scale).round() as i32,
        y: (rect.min.y * scale).round() as i32,
    };
    unsafe {
        let _ = ClientToScreen(HWND(hwnd as *mut _), &mut p);
    }
    [
        p.x,
        p.y,
        (rect.width() * scale).round() as i32,
        (rect.height() * scale).round() as i32,
    ]
}
pub fn initial_size(requested: [f32; 2]) -> [f32; 2] {
    let mut work = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some((&mut work as *mut RECT).cast()),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() } as f32 / 96.;
    let w = if work.right > work.left {
        work.right - work.left
    } else {
        unsafe { GetSystemMetrics(SM_CXSCREEN) }
    };
    let h = if work.bottom > work.top {
        work.bottom - work.top
    } else {
        unsafe { GetSystemMetrics(SM_CYSCREEN) }
    };
    [
        requested[0].min(w as f32 / dpi - 24.),
        requested[1].min(h as f32 / dpi - 24.),
    ]
}
pub fn open(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let text: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(text.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err(format!("无法打开：{}", path.display()))
    } else {
        Ok(())
    }
}
pub fn pick_directory(screenshot: bool, sender: mpsc::Sender<BackgroundEvent>) {
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(if screenshot {
                "选择截图保存目录"
            } else {
                "选择录像保存目录"
            })
            .pick_folder()
        {
            let _ = sender.send(BackgroundEvent::Directory { screenshot, path });
        }
    });
}
pub fn export_logs(sender: mpsc::Sender<BackgroundEvent>) {
    std::thread::spawn(move || {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("选择诊断日志导出位置")
            .pick_folder()
        else {
            return;
        };
        let output = dir.join(format!(
            "iPhonePlay-diagnostics-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        let result = (|| -> Result<(), String> {
            std::fs::create_dir(&output).map_err(|e| e.to_string())?;
            let logs = iphoneplay::settings::data_dir().join("logs");
            if logs.is_dir() {
                for entry in std::fs::read_dir(logs).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.file_type().map_err(|e| e.to_string())?.is_file() {
                        std::fs::copy(entry.path(), output.join(entry.file_name()))
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            std::fs::write(output.join("environment.txt"),format!("iPhonePlay {}\nRust 1.96.0\nCore ABI 18\nUpstream 31acd8a78e7038d46c7e1a6ee987b9035cf36dde\nMissing: {:?}\n",env!("CARGO_PKG_VERSION"),iphoneplay::native::missing_dependencies(&runtime_dir()))).map_err(|e|e.to_string())?;
            Ok(())
        })();
        let event = match result {
            Ok(()) => BackgroundEvent::Exported(output),
            Err(e) => BackgroundEvent::Failed(format!("日志导出失败：{e}")),
        };
        let _ = sender.send(event);
    });
}
