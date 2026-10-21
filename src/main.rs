#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod floating;
mod platform;
mod ui;

use eframe::egui;
use iphoneplay::settings::{Settings, Theme, data_dir};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut instance = if args.iter().any(|a| a.starts_with("--demo=")) {
        None
    } else {
        match platform::Instance::acquire() {
            Ok(Some(i)) => Some(i),
            Ok(None) => return,
            Err(e) => {
                rfd::MessageDialog::new()
                    .set_title("iPhonePlay")
                    .set_description(e)
                    .show();
                return;
            }
        }
    };
    let folder = data_dir();
    let _ = std::fs::create_dir_all(folder.join("logs"));
    // Before any worker/thread starts, redirect unchanged upstream Core logs.
    unsafe {
        std::env::set_var("IPHONE_MIRROR_LOG_FILE", folder.join("logs/core.log"));
    }
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("logs/iphoneplay.log"))
    {
        let _ = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter("iphoneplay=debug,iPhonePlay=debug,wgpu=warn")
            .with_writer(std::sync::Mutex::new(file))
            .try_init();
    }
    let load = Settings::load(&folder.join("settings.json"));
    let mut settings = load.as_ref().cloned().unwrap_or_default();
    if let Err(e) = load {
        tracing::error!(%e,"configuration load");
    }
    if let Some(theme) = args.iter().find_map(|a| a.strip_prefix("--theme=")) {
        settings.theme = match theme {
            "dark" => Theme::Dark,
            "light" => Theme::Light,
            _ => Theme::System,
        };
    }
    if let Some(scale) = args
        .iter()
        .find_map(|a| a.strip_prefix("--scale="))
        .and_then(|s| s.parse::<f32>().ok())
    {
        settings.scale = scale;
    }
    if let Some(size) = args.iter().find_map(|a| a.strip_prefix("--size=")) {
        let parts: Vec<f32> = size.split('x').filter_map(|n| n.parse().ok()).collect();
        if parts.len() == 2 {
            settings.window_size = [parts[0], parts[1]];
        }
    }
    settings.validate();
    let glow = args.iter().any(|s| s == "--renderer=glow");
    let hidden_capture = args.iter().any(|s| s == "--hidden-capture")
        && args.iter().any(|s| s.starts_with("--demo="))
        && args.iter().any(|s| s.starts_with("--screenshot="));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("iPhonePlay")
            .with_app_id("iPhonePlay.Desktop")
            .with_decorations(false)
            .with_visible(!hidden_capture)
            .with_inner_size(platform::initial_size(if settings.remember_window {
                settings.window_size
            } else {
                [1280., 900.]
            }))
            .with_min_inner_size([960., 640.])
            .with_icon(platform::app_icon()),
        renderer: if glow {
            eframe::Renderer::Glow
        } else {
            eframe::Renderer::Wgpu
        },
        centered: true,
        ..Default::default()
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        renderer = if glow { "glow" } else { "wgpu" },
        "starting iPhonePlay"
    );
    if let Err(e) = eframe::run_native(
        "iPhonePlay",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, settings, &args)))),
    ) {
        tracing::error!(%e,"UI initialization failed");
        // winit event loops cannot reliably be recreated on the same thread;
        // use a fresh process for the supported OpenGL fallback.
        if !glow {
            drop(instance.take());
            if let Ok(exe) = std::env::current_exe() {
                let mut cmd = std::process::Command::new(exe);
                cmd.args(std::env::args().skip(1)).arg("--renderer=glow");
                let _ = cmd.spawn();
            }
        } else {
            rfd::MessageDialog::new()
                .set_title("iPhonePlay 无法启动")
                .set_description(format!(
                    "图形界面初始化失败：{e}\n日志：{}",
                    folder.join("logs").display()
                ))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
    }
}
