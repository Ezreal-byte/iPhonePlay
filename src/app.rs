use crate::{
    floating::FloatingWindow,
    platform::{self, BackgroundEvent, Tray},
    ui::{Action, UiModel, theme},
};
use eframe::{egui, glow};
use iphoneplay::{
    control::{ControlBackend, ControlCommand, ControlEvent, ControlState},
    media::{MediaEvent, Recorder, RecordingOptions},
    native::{self, Command, Device, Event, NativeBackend, NativeSession, PreviewWindow},
    settings::{Settings, data_dir},
    state::{CaptureState, Page, RecordingState, SessionGate, Source, Transition},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

pub struct App {
    pub model: UiModel,
    backend: Option<NativeBackend>,
    control: ControlBackend,
    recorder: Recorder,
    session: Option<Arc<NativeSession>>,
    gate: SessionGate,
    device: Option<Device>,
    usb_devices: Vec<Device>,
    preferred_device: Option<String>,
    suppressed: HashSet<String>,
    preview: Option<PreviewWindow>,
    floating: Option<FloatingWindow>,
    hwnd: isize,
    tray: Option<Tray>,
    hidden: bool,
    runtime: PathBuf,
    settings_path: PathBuf,
    settings_read_only: bool,
    last_saved: Settings,
    settings_dirty: Option<Instant>,
    background_tx: mpsc::Sender<BackgroundEvent>,
    background_rx: mpsc::Receiver<BackgroundEvent>,
    disconnect_requested: bool,
    disconnect_started: Option<Instant>,
    exit_allowed: bool,
    shutdown_requested: bool,
    shutdown_started: Option<Instant>,
    native_shutdown_complete: bool,
    native_shutdown_failed: bool,
    control_shutdown_complete: bool,
    was_foreground: bool,
    video_focus: bool,
    screenshot_path: Option<PathBuf>,
    capture_frame: u32,
    screenshot_requested: bool,
    quit_after_capture: bool,
    hidden_capture: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, args: &[String]) -> Self {
        theme::configure(&cc.egui_ctx);
        let hwnd = cc
            .window_handle()
            .ok()
            .and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
                _ => None,
            })
            .unwrap_or(0);
        let runtime = platform::runtime_dir();
        let mut model = UiModel::new(settings.clone());
        let settings_read_only = Settings::load(&data_dir().join("settings.json")).is_err();
        if settings_read_only {
            model.notify(
                "现有配置无法读取，已使用默认值；原文件已保留，不会覆盖",
                None,
            );
        }
        let demo_arg = args.iter().find_map(|s| s.strip_prefix("--demo="));
        model.demo = demo_arg.is_some();
        for (name, bytes, target) in [
            (
                "theme-system",
                include_bytes!("../assets/theme-system.png").as_slice(),
                1,
            ),
            (
                "theme-light",
                include_bytes!("../assets/theme-light.png").as_slice(),
                1,
            ),
            (
                "theme-dark",
                include_bytes!("../assets/theme-dark.png").as_slice(),
                1,
            ),
            (
                "welcome",
                include_bytes!("../assets/welcome.png").as_slice(),
                0,
            ),
            (
                "demo-source",
                include_bytes!("../design/review-v2/images/02-mirror-dark.png").as_slice(),
                2,
            ),
            (
                "app-icon",
                include_bytes!("../assets/app.png").as_slice(),
                3,
            ),
        ] {
            if target == 2 && !model.demo {
                continue;
            }
            if let Ok(img) = image::load_from_memory(bytes) {
                let img = img.into_rgba8();
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                let texture = cc
                    .egui_ctx
                    .load_texture(name, color, egui::TextureOptions::LINEAR);
                match target {
                    0 => model.welcome_texture = Some(texture),
                    1 => model.theme_textures.push(texture),
                    2 => model.demo_texture = Some(texture),
                    _ => model.app_texture = Some(texture),
                }
            }
        }
        if let Some(demo) = demo_arg {
            model.receiver_ready = true;
            model.capture = CaptureState::Waiting;
            model.device_name = "演示 iPhone".into();
            model.source_size = [1170, 2532];
            model.page = match demo {
                "workspace" | "control" => Page::Workspace,
                "settings" => Page::Settings,
                "usb" => Page::Usb,
                "recovery" => Page::Recovery,
                _ => Page::Welcome,
            };
            if matches!(model.page, Page::Workspace | Page::Recovery) {
                model.capture = if model.page == Page::Recovery {
                    CaptureState::Interrupted
                } else {
                    CaptureState::Streaming
                };
            }
            model.control_open = demo == "control";
            if args.iter().any(|arg| arg == "--open-guide") {
                if model.capture == CaptureState::Streaming {
                    model.control_open = true;
                    model.control_expand_request = true;
                } else {
                    model.help = Some("蓝牙反控连接");
                }
            }
            model.settings_tab = args
                .iter()
                .find_map(|s| s.strip_prefix("--settings-tab="))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0)
                .min(4);
        }
        let backend = if model.demo {
            None
        } else {
            Some(NativeBackend::spawn(
                runtime.clone(),
                settings.receiver_name.clone(),
                capture_options(&settings),
            ))
        };
        let preview = if hwnd != 0 && !model.demo {
            match PreviewWindow::new(hwnd) {
                Ok(p) => {
                    if let Some(b) = &backend {
                        let _ = b.send(Command::AttachPreview { hwnd: p.hwnd() });
                    }
                    Some(p)
                }
                Err(e) => {
                    model.notify(e, None);
                    None
                }
            }
        } else {
            None
        };
        let control = ControlBackend::new();
        control.send(ControlCommand::SetWindow(hwnd));
        control.send(ControlCommand::SetSensitivity {
            mouse: settings.mouse_sensitivity,
            wheel: settings.wheel_sensitivity,
        });
        let tray = if model.demo {
            None
        } else {
            match Tray::new(cc.egui_ctx.clone()) {
                Ok(t) => Some(t),
                Err(e) => {
                    model.notify(format!("托盘不可用，关闭窗口将安全退出：{e}"), None);
                    None
                }
            }
        };
        let (background_tx, background_rx) = mpsc::channel();
        Self {
            model,
            backend,
            control,
            recorder: Recorder::new(platform::ffmpeg(&runtime)),
            session: None,
            gate: SessionGate::default(),
            device: None,
            usb_devices: vec![],
            preferred_device: None,
            suppressed: HashSet::new(),
            preview,
            floating: None,
            hwnd,
            tray,
            hidden: false,
            runtime,
            settings_path: data_dir().join("settings.json"),
            settings_read_only,
            last_saved: settings,
            settings_dirty: None,
            background_tx,
            background_rx,
            disconnect_requested: false,
            disconnect_started: None,
            exit_allowed: false,
            shutdown_requested: false,
            shutdown_started: None,
            native_shutdown_complete: false,
            native_shutdown_failed: false,
            control_shutdown_complete: false,
            was_foreground: true,
            video_focus: false,
            screenshot_path: args
                .iter()
                .find_map(|s| s.strip_prefix("--screenshot=").map(PathBuf::from)),
            capture_frame: 0,
            screenshot_requested: false,
            quit_after_capture: args.iter().any(|s| s == "--quit-after-capture"),
            hidden_capture: args.iter().any(|s| s == "--hidden-capture") && demo_arg.is_some(),
        }
    }
    fn command(&mut self, c: Command) {
        if let Some(b) = &self.backend
            && let Err(e) = b.send(c)
        {
            self.model.notify(e, None);
        }
    }
    fn release(&mut self, reason: &str) {
        self.control.release(reason);
        self.video_focus = false;
        self.control.send(ControlCommand::Focus {
            session_id: self.gate.generation(),
            focused: false,
        });
        self.model.control_active = false;
    }
    fn input_window(&self) -> isize {
        self.floating
            .as_ref()
            .map_or(self.hwnd, FloatingWindow::hwnd)
    }
    fn dock_preview(&mut self) {
        if self.floating.is_none() {
            return;
        }
        self.release("画面返回主窗口，键鼠已释放");
        if let Some(preview) = &mut self.preview {
            preview.set_visible(false);
            if let Err(error) = preview.reparent(self.hwnd) {
                self.model.notify(error, None);
                return;
            }
        }
        self.floating.take();
        self.model.floating = false;
        self.control.send(ControlCommand::SetWindow(self.hwnd));
    }
    fn update_floating_preview(&mut self, ctx: &egui::Context) {
        let Some(floating) = &mut self.floating else {
            return;
        };
        if floating.take_close_requested() {
            self.dock_preview();
            self.hidden = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            return;
        }
        let _ = floating.take_layout_changed();
        let _ = floating.take_video_clicked();
        let _ = floating.set_topmost(self.model.settings.floating_always_on_top);
        let visible = self.model.capture == CaptureState::Streaming
            && self.model.pending.is_none()
            && !floating.is_minimized();
        let rect = crate::floating::fit_video_rect(floating.client_size(), self.model.source_size);
        let parent = floating.hwnd();
        if let Some(preview) = &mut self.preview {
            if visible {
                if let Err(error) = preview.set_rect(rect[0], rect[1], rect[2], rect[3]) {
                    tracing::error!(%error, "floating preview layout");
                }
                let radius = if self.model.settings.preview_rounded {
                    (self.model.settings.preview_corner_radius * floating.dpi() as f32 / 96.)
                        .round() as u32
                } else {
                    0
                };
                if let Err(error) = preview.set_corner_radius(radius) {
                    tracing::error!(%error, "preview rounding");
                }
            }
            preview.set_visible(visible);
        }
        let video = egui::Rect::from_min_size(
            egui::pos2(rect[0] as f32, rect[1] as f32),
            egui::vec2(rect[2] as f32, rect[3] as f32),
        );
        let [x, y, width, height] = if visible {
            platform::screen_rect(parent, video, 1.)
        } else {
            [0; 4]
        };
        self.control.send(ControlCommand::SetVideoRect {
            x,
            y,
            width,
            height,
        });
    }
    fn connect(&mut self, device: Device) {
        if self.model.pending.is_some() {
            return;
        }
        if let Some(generation) = self.gate.reserve(device.id.clone()) {
            self.model.capture = CaptureState::Connecting;
            self.model.status = "正在连接 iPhone，等待第一帧画面…".into();
            self.preferred_device = Some(device.id.clone());
            self.device = Some(device.clone());
            self.command(Command::Connect { device, generation });
        }
    }
    fn request_transition(&mut self, transition: Transition) {
        if self.model.pending.is_some() {
            return;
        }
        self.release("正在结束当前会话");
        self.model.pending = Some(transition);
        self.disconnect_requested = false;
        self.disconnect_started = None;
        if self.recorder.is_busy() || self.model.recording_busy() {
            self.recorder.stop();
            self.model.recording = RecordingState::Finalizing;
        }
    }
    fn advance_transition(&mut self, ctx: &egui::Context) {
        if self.recorder.is_busy() || self.model.recording_busy() {
            return;
        }
        let Some(transition) = self.model.pending else {
            return;
        };
        if self.gate.occupied() && !self.disconnect_requested {
            self.disconnect_requested = true;
            self.disconnect_started = Some(Instant::now());
            if let Some(d) = &self.device {
                self.suppressed.insert(d.id.clone());
            }
            let result = self
                .backend
                .as_ref()
                .ok_or_else(|| "投屏服务已停止".to_owned())
                .and_then(|backend| {
                    backend.send(Command::Disconnect {
                        generation: self.gate.generation(),
                    })
                });
            if let Err(error) = result {
                tracing::error!(%error, "disconnect command failed; retiring local session");
                self.model.notify(
                    format!("投屏服务未能确认断开：{error}，正在清理本地会话。"),
                    None,
                );
                self.complete_disconnect();
            }
            return;
        }
        if self.gate.occupied() {
            if self
                .disconnect_started
                .is_some_and(|started| started.elapsed() >= Duration::from_secs(15))
            {
                let message = "投屏服务在 15 秒内未确认断开，已释放键鼠，正在强制清理当前会话。";
                tracing::error!("{message}");
                self.model.notify(message, None);
                self.model.status = message.into();
                self.model.capture = CaptureState::Failed;
                self.complete_disconnect();
                if transition != Transition::Exit {
                    // A stuck backend cannot safely accept a replacement
                    // session. Return to recovery instead of claiming success.
                    self.model.pending = None;
                    self.model.page = Page::Recovery;
                    self.control.send(ControlCommand::Stop);
                    return;
                }
            } else {
                return;
            }
        }
        if transition == Transition::Exit {
            if !self.shutdown_requested {
                self.shutdown_requested = true;
                self.shutdown_started = Some(Instant::now());
                self.control.request_shutdown();
                self.native_shutdown_complete = self
                    .backend
                    .as_ref()
                    .is_none_or(|backend| backend.send(Command::Shutdown).is_err());
            }
            self.control_shutdown_complete = self.control.shutdown_complete();
            // A timeout is a diagnosed failure, never a successful native ack.
            if !self.native_shutdown_complete
                && !self.native_shutdown_failed
                && self
                    .shutdown_started
                    .is_some_and(|t| t.elapsed() > Duration::from_secs(15))
            {
                self.native_shutdown_failed = true;
                let message = "投屏服务退出超时。录像已完成处理、键鼠已释放，将结束应用进程。";
                tracing::error!("{message}");
                self.model.notify(message, None);
                self.model.status = message.into();
            }
            self.exit_allowed = self.control_shutdown_complete
                && (self.native_shutdown_complete || self.native_shutdown_failed);
            if self.exit_allowed {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        self.model.pending = None;
        self.model.control_open = false;
        self.control.send(ControlCommand::Stop);
        self.preferred_device = None;
        if let Transition::Switch(source) = transition {
            self.model.source = source;
        }
        self.model.page = if self.model.source == Source::Usb {
            Page::Usb
        } else {
            Page::Welcome
        };
        self.model.capture = CaptureState::Waiting;
        self.model.status = "等待 iPhone 连接".into();
        if self.model.source == Source::Usb {
            self.command(Command::RefreshDevices);
        }
    }
    fn complete_disconnect(&mut self) {
        self.dock_preview();
        if let Some(session) = self.session.take() {
            // The last Arc may invoke native destroy/shutdown. Never perform
            // that potentially blocking FFI teardown on the egui thread.
            if let Err(error) = std::thread::Builder::new()
                .name("iphoneplay-session-cleanup".into())
                .spawn(move || drop(session))
            {
                tracing::error!(%error, "cannot spawn session cleanup thread");
            }
        }
        self.device = None;
        self.gate.invalidate();
        self.disconnect_requested = false;
        self.disconnect_started = None;
        if let Some(p) = &mut self.preview {
            p.set_visible(false);
        }
    }
    fn interrupted(&mut self, message: String) {
        if matches!(
            self.model.capture,
            CaptureState::Interrupted | CaptureState::Failed
        ) {
            return;
        }
        self.release("投屏断线，键鼠已释放");
        self.dock_preview();
        self.model.capture = CaptureState::Interrupted;
        self.model.status = message;
        self.model.page = Page::Recovery;
        if self.recorder.is_busy() {
            self.recorder.stop();
            self.model.recording = RecordingState::Finalizing;
        }
        self.control.send(ControlCommand::Stop);
    }
    fn poll_native(&mut self) {
        loop {
            let event = self.backend.as_ref().and_then(|b| b.try_recv());
            let Some(event) = event else {
                break;
            };
            match event {
                Event::Ready { receiver_ready } => {
                    self.model.receiver_ready = receiver_ready;
                    if self.model.capture == CaptureState::Preparing {
                        self.model.capture = CaptureState::Waiting;
                        self.model.status = "接收服务准备中…".into();
                    }
                }
                Event::ReceiverStatus { running, ready } => {
                    self.model.receiver_ready = ready;
                    if !running {
                        if self.session.is_some() && self.model.source == Source::AirPlay {
                            self.interrupted("AirPlay 无线宿主已停止，请重新准备接收服务".into());
                        } else {
                            self.model.status = "接收服务未运行，可打开连接指引重试".into();
                        }
                    } else if ready && !self.gate.occupied() {
                        self.model.status = "接收端已就绪 · 等待 iPhone 连接".into();
                        if self.model.capture == CaptureState::Failed {
                            self.model.capture = CaptureState::Waiting;
                        }
                    }
                }
                Event::Devices {
                    usb,
                    wireless,
                    environment,
                } => {
                    tracing::debug!(usb=usb.len(),wireless=wireless.len(),diagnostic=%environment.diagnostic,"discovery");
                    self.model.usb_devices = usb
                        .iter()
                        .map(|d| {
                            (
                                if d.name.is_empty() {
                                    "iPhone".into()
                                } else {
                                    d.name.clone()
                                },
                                if d.driver_available == Some(false) {
                                    "缺少投屏驱动，请打开驱动管理器".into()
                                } else if !d.trusted && !d.paired {
                                    "请解锁并信任这台电脑".into()
                                } else {
                                    d.message.clone()
                                },
                                d.driver_available == Some(true) && (d.trusted || d.paired),
                            )
                        })
                        .collect();
                    self.usb_devices = usb;
                    let live: HashSet<_> = wireless.iter().map(|d| d.id.clone()).collect();
                    self.suppressed.retain(|id| live.contains(id));
                    if self.model.source == Source::AirPlay
                        && !self.gate.occupied()
                        && self.model.pending.is_none()
                    {
                        let next = wireless.into_iter().find(|d| {
                            d.state == 4
                                && !self.suppressed.contains(&d.id)
                                && self.preferred_device.as_ref().is_none_or(|id| id == &d.id)
                        });
                        if let Some(d) = next {
                            self.connect(d);
                        }
                    }
                }
                Event::SessionStarted {
                    generation,
                    device,
                    session,
                } => {
                    if !self.gate.accepts(generation) {
                        continue;
                    }
                    self.session = Some(session);
                    self.model.device_name = device.name.clone();
                    self.model.source = if device.kind == native::ConnectionKind::Usb {
                        Source::Usb
                    } else {
                        Source::AirPlay
                    };
                    self.device = Some(device);
                }
                Event::SessionStatus { generation, status } => {
                    if !self.gate.accepts(generation) {
                        continue;
                    }
                    if status.has_video() && self.model.pending.is_none() {
                        if self.model.capture != CaptureState::Streaming {
                            self.model.capture = CaptureState::Streaming;
                            if self.model.page != Page::Settings {
                                self.model.page = Page::Workspace;
                            }
                        }
                        self.model.source_size = [status.width, status.height];
                        self.model.source_fps = status.fps;
                    } else if status.is_terminal() && self.model.pending.is_none() {
                        self.interrupted(status.message.clone());
                    }
                    self.model.status = status.message;
                }
                Event::SessionStopped { generation } => {
                    if self.gate.accepts(generation) {
                        self.complete_disconnect();
                    }
                }
                Event::Error {
                    generation,
                    message,
                } => {
                    if generation.is_some_and(|g| !self.gate.accepts(g)) {
                        continue;
                    }
                    tracing::error!(%message,"capture");
                    if generation.is_some() {
                        if self.model.capture == CaptureState::Streaming {
                            self.interrupted(message.clone());
                        } else if self.session.is_none() {
                            self.model.capture = CaptureState::Failed;
                            self.model.status = message.clone();
                            if let Some(d) = &self.device {
                                self.suppressed.insert(d.id.clone());
                            }
                            self.gate.invalidate();
                            self.device = None;
                        }
                    } else if self.session.is_none() {
                        self.model.capture = CaptureState::Failed;
                        self.model.status = message.clone();
                    }
                    self.model.notify(message, None);
                }
                Event::ShutdownComplete => {
                    self.native_shutdown_complete = true;
                }
            }
        }
        if self.model.capture == CaptureState::Interrupted
            && !self.recorder.is_busy()
            && !self.model.recording_busy()
            && self.gate.occupied()
            && !self.disconnect_requested
        {
            self.disconnect_requested = true;
            self.disconnect_started = Some(Instant::now());
            if let Some(d) = &self.device {
                self.suppressed.insert(d.id.clone());
            }
            let result = self
                .backend
                .as_ref()
                .ok_or_else(|| "投屏服务已停止".to_owned())
                .and_then(|backend| {
                    backend.send(Command::Disconnect {
                        generation: self.gate.generation(),
                    })
                });
            if let Err(error) = result {
                tracing::error!(%error, "disconnect after interruption could not be sent");
                self.model
                    .notify(format!("连接已中断，投屏服务未确认清理：{error}"), None);
                self.complete_disconnect();
            }
        }
        if self.model.capture == CaptureState::Interrupted
            && self.model.pending.is_none()
            && self.gate.occupied()
            && self
                .disconnect_started
                .is_some_and(|started| started.elapsed() >= Duration::from_secs(15))
        {
            let message = "连接已中断，投屏服务清理超时。键鼠已释放，请退出并重新打开 iPhonePlay。";
            tracing::error!("{message}");
            self.model.notify(message, None);
            self.model.status = message.into();
            self.model.capture = CaptureState::Failed;
            self.complete_disconnect();
        }
    }
    fn poll_media(&mut self) {
        while let Some(event) = self.recorder.try_event() {
            match &event {
                MediaEvent::ScreenshotSaved { path, .. } => {
                    self.model.notify("截图已保存", Some(path.clone()));
                    continue;
                }
                MediaEvent::ScreenshotFailed { message, .. } => {
                    self.model.notify(message.clone(), None);
                    continue;
                }
                _ => {}
            }
            if !self.gate.accepts(event.generation()) {
                continue;
            }
            match event {
                MediaEvent::Starting { .. } => self.model.recording = RecordingState::Starting,
                MediaEvent::Started {
                    started_at,
                    encoder,
                    ..
                } => {
                    self.model.recording = RecordingState::Recording;
                    self.model.record_started = Some(started_at);
                    tracing::info!(%encoder,"recording started");
                }
                MediaEvent::Finalizing { .. } => self.model.recording = RecordingState::Finalizing,
                MediaEvent::Saved { path, .. } => {
                    self.model.recording = RecordingState::Completed;
                    self.model.record_started = None;
                    self.model.notify("录像已保存", Some(path));
                }
                MediaEvent::Failed {
                    message,
                    partial_path,
                    ..
                } => {
                    self.model.recording = RecordingState::Failed;
                    self.model.record_started = None;
                    self.model.notify(message.clone(), partial_path);
                    tracing::error!(%message,"recording");
                }
                MediaEvent::Cancelled { .. } => {
                    self.model.recording = RecordingState::Idle;
                    self.model.record_started = None;
                }
                MediaEvent::ScreenshotSaved { path, .. } => {
                    self.model.notify("截图已保存", Some(path))
                }
                MediaEvent::ScreenshotFailed { message, .. } => self.model.notify(message, None),
            }
        }
    }
    fn poll_control(&mut self) {
        while let Some(event) = self.control.try_event() {
            match event {
                ControlEvent::Snapshot(s) => {
                    if s.session_id != 0 && !self.gate.accepts(s.session_id) {
                        continue;
                    }
                    self.model.control_ready = s.is_ready();
                    self.model.control_state = s.state;
                    self.model.control_active = s.state == ControlState::Active
                        && self.model.control_ready
                        && self.control.is_active();
                    self.model.control_status = s.message;
                    self.model.bluetooth_name = s.advertised_name;
                    self.model.clients = s.clients.into_iter().map(|c| (c.id, c.name)).collect();
                    self.model.bound_client = s.bound_client.clone();
                    self.model.settings.bound_client = s.bound_client;
                }
                ControlEvent::Error(e) => {
                    self.model.control_active = false;
                    self.model.control_ready = false;
                    self.model.notify(e, None);
                }
            }
        }
    }
    fn process_action(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::ShowSettings => {
                self.release("进入设置，键鼠已释放");
                self.model.page = Page::Settings;
            }
            Action::Back => {
                self.model.page = if self.model.capture == CaptureState::Streaming {
                    Page::Workspace
                } else if self.model.capture == CaptureState::Interrupted {
                    Page::Recovery
                } else if self.model.source == Source::Usb {
                    Page::Usb
                } else {
                    Page::Welcome
                }
            }
            Action::ConnectUsb(index) => {
                if let Some(device) = self.usb_devices.get(index).cloned() {
                    self.connect(device);
                }
            }
            Action::RefreshDevices => self.command(Command::RefreshDevices),
            Action::DriverManager => {
                self.release("打开驱动管理器");
                let path = self.runtime.join("iPhoneMirror.Driver.exe");
                if !path.is_file() {
                    self.model
                        .notify("未找到独立驱动管理器，请使用完整发行包", None);
                } else if let Err(e) = platform::open(&path) {
                    self.model.notify(e, None);
                }
            }
            Action::Transition(t) => self.request_transition(t),
            Action::StartControl => {
                if self.model.capture == CaptureState::Streaming
                    && !self.model.demo
                    && let Some(d) = &self.device
                {
                    self.control.send(ControlCommand::Start {
                        session_id: self.gate.generation(),
                        device_id: d.id.clone(),
                    });
                }
            }
            Action::ShowControlGuide => {
                self.model.control_open = true;
                self.model.control_expand_request = true;
                if self.model.capture == CaptureState::Streaming {
                    self.model.page = Page::Workspace;
                    self.process_action(Action::StartControl, ctx);
                } else {
                    self.model.help = Some("蓝牙反控连接");
                }
            }
            Action::OpenBluetoothSettings => {
                self.release("打开蓝牙设置，键鼠已释放");
                if let Err(error) = platform::open(std::path::Path::new("ms-settings:bluetooth")) {
                    self.model.notify(error, None);
                }
            }
            Action::ToggleFloating => {
                if self.floating.is_some() {
                    self.dock_preview();
                } else if self.model.capture == CaptureState::Streaming && !self.model.demo {
                    self.release("切换悬浮窗口，键鼠已释放");
                    match FloatingWindow::new(
                        self.hwnd,
                        "iPhonePlay · 悬浮投屏",
                        self.model.source_size,
                    ) {
                        Ok(mut floating) => {
                            let _ = floating
                                .set_title(&format!("{} · iPhonePlay", self.model.device_name));
                            let _ =
                                floating.set_topmost(self.model.settings.floating_always_on_top);
                            if let Some(preview) = &mut self.preview {
                                preview.set_visible(false);
                                match preview.reparent(floating.hwnd()) {
                                    Ok(()) => {
                                        self.control
                                            .send(ControlCommand::SetWindow(floating.hwnd()));
                                        self.floating = Some(floating);
                                        self.model.floating = true;
                                        self.update_floating_preview(ctx);
                                    }
                                    Err(error) => self.model.notify(error, None),
                                }
                            }
                        }
                        Err(error) => self.model.notify(error, None),
                    }
                }
            }
            Action::ConfirmClient(id) => {
                self.control.send(ControlCommand::ConfirmClient(id));
            }
            Action::ActivateControl => {
                if !self.model.demo
                    && self.model.capture == CaptureState::Streaming
                    && self.model.page == Page::Workspace
                    && self.model.help.is_none()
                    && self.model.control_ready
                {
                    let Some(preview) = &self.preview else {
                        return;
                    };
                    let preview_hwnd = windows::Win32::Foundation::HWND(preview.hwnd() as *mut _);
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(
                            windows::Win32::Foundation::HWND(self.input_window() as *mut _),
                        );
                        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(
                            preview_hwnd,
                        ));
                    }
                    if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() }
                        != preview_hwnd
                    {
                        self.release("无法将输入焦点移至投屏画面，键鼠保持在本机。");
                        return;
                    }
                    self.video_focus = true;
                    self.control.send(ControlCommand::Focus {
                        session_id: self.gate.generation(),
                        focused: true,
                    });
                    self.control.send(ControlCommand::Activate);
                }
            }
            Action::ReleaseControl => self.release("键鼠控制已暂停，按开始控制可重新接管"),
            Action::ForgetBinding => {
                self.release("已解除应用内绑定");
                self.control.send(ControlCommand::ForgetBinding);
                self.model.settings.bound_client = None;
            }
            Action::StartRecording => {
                if let Some(s) = &self.session {
                    let options = RecordingOptions {
                        directory: self.model.settings.recording_dir.clone(),
                        max_long_edge: self.model.settings.recording_long_edge,
                        fps: self.model.settings.recording_fps,
                        record_audio: self.model.settings.record_audio,
                        bitrate_kbps: self.model.settings.recording_bitrate,
                    };
                    match self
                        .recorder
                        .start(s.clone(), options, self.gate.generation())
                    {
                        Ok(()) => self.model.recording = RecordingState::Starting,
                        Err(e) => self.model.notify(e, None),
                    }
                }
            }
            Action::StopRecording => {
                self.recorder.stop();
                self.model.recording = RecordingState::Finalizing;
            }
            Action::Screenshot => {
                if let Some(s) = &self.session
                    && let Err(e) = self.recorder.screenshot(
                        s.clone(),
                        self.model.settings.screenshot_dir.clone(),
                        self.gate.generation(),
                    )
                {
                    self.model.notify(e, None);
                }
            }
            Action::RestartReceiver => {
                if self.model.demo {
                    return;
                }
                if self.gate.occupied() || self.recorder.is_busy() || self.model.pending.is_some() {
                    self.model
                        .notify("请先结束当前投屏与录制，再重新准备接收服务", None);
                    return;
                }
                self.model.receiver_ready = false;
                self.model.status = "正在重新准备接收服务…".into();
                self.model.capture = CaptureState::Preparing;
                let command = Command::RestartReceiver {
                    name: self.model.settings.receiver_name.clone(),
                };
                if self
                    .backend
                    .as_ref()
                    .is_none_or(|b| b.send(command).is_err())
                {
                    self.backend = Some(NativeBackend::spawn(
                        self.runtime.clone(),
                        self.model.settings.receiver_name.clone(),
                        capture_options(&self.model.settings),
                    ));
                    if let Some(p) = &self.preview {
                        self.command(Command::AttachPreview { hwnd: p.hwnd() });
                    }
                }
            }
            Action::ExportLogs => {
                self.release("导出日志");
                platform::export_logs(self.background_tx.clone());
            }
            Action::OpenPath(path) => {
                let resolved = if path.is_relative() {
                    self.runtime.join(path)
                } else {
                    path
                };
                let target = if resolved.is_file()
                    && resolved
                        .extension()
                        .is_some_and(|e| e == "mp4" || e == "png")
                {
                    resolved.parent().unwrap_or(&resolved).to_path_buf()
                } else {
                    resolved
                };
                if let Err(e) = platform::open(&target) {
                    self.model.notify(e, None);
                }
            }
            Action::PickDirectory(screenshot) => {
                self.release("选择保存目录");
                platform::pick_directory(screenshot, self.background_tx.clone());
            }
            Action::Minimize => {
                self.release("窗口已最小化");
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            Action::Close => {
                self.release("主窗口已关闭");
                if self.model.settings.close_to_tray && self.tray.is_some() && !self.model.demo {
                    if self.model.settings.tray_notice_seen {
                        self.process_action(Action::HideToTray, ctx);
                    } else {
                        self.model.help = Some("后台继续运行");
                    }
                } else {
                    self.request_transition(Transition::Exit);
                }
            }
            Action::HideToTray => {
                self.release("窗口已收起到托盘");
                self.hidden = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            Action::Fullscreen => {
                self.release("切换全屏");
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
            }
            Action::WindowMaximize => {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            Action::InspectDependencies => {
                let mut lines = vec![format!(
                    "iPhonePlay {} · Core ABI 18",
                    env!("CARGO_PKG_VERSION")
                )];
                let missing = native::missing_dependencies(&self.runtime);
                if missing.is_empty() {
                    lines.push("投屏核心与无线宿主文件齐全".into());
                } else {
                    lines.extend(missing.iter().map(|p| format!("缺少：{}", p.display())));
                }
                lines.push(format!(
                    "FFmpeg：{}",
                    if platform::ffmpeg(&self.runtime).is_file() {
                        "已安装"
                    } else {
                        "缺失"
                    }
                ));
                lines.push(format!(
                    "独立驱动管理器：{}",
                    if self.runtime.join("iPhoneMirror.Driver.exe").is_file() {
                        "已安装"
                    } else {
                        "缺失"
                    }
                ));
                lines.push(format!("服务状态：{}", self.model.status));
                self.model.dependency_report = Some(lines.join("\n"));
                self.model.help = Some("依赖检查");
                self.release("依赖检查");
            }
        }
    }
    fn persist(&mut self, force: bool) {
        if self.model.demo || self.settings_read_only {
            return;
        }
        if self.model.settings != self.last_saved {
            self.settings_dirty.get_or_insert_with(Instant::now);
        }
        if (force
            || self
                .settings_dirty
                .is_some_and(|t| t.elapsed() > Duration::from_millis(500)))
            && self.model.settings != self.last_saved
        {
            self.model.settings.validate();
            match self.model.settings.save(&self.settings_path) {
                Ok(()) => {
                    let s = &self.model.settings;
                    self.command(Command::SetAudio {
                        enabled: s.play_audio,
                        volume: s.volume,
                    });
                    let s = &self.model.settings;
                    self.command(Command::SetPreview {
                        width: s.preview_long_edge,
                        height: s.preview_long_edge,
                        fps: s.preview_fps,
                    });
                    self.command(Command::SetUsbMode(self.model.settings.usb_mode));
                    self.control.send(ControlCommand::SetSensitivity {
                        mouse: self.model.settings.mouse_sensitivity,
                        wheel: self.model.settings.wheel_sensitivity,
                    });
                    self.last_saved = self.model.settings.clone();
                    self.settings_dirty = None;
                }
                Err(e) => {
                    self.model.notify(format!("设置未保存：{e}"), None);
                    self.settings_dirty = Some(Instant::now() + Duration::from_secs(5));
                }
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hidden_capture && !self.screenshot_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
        self.poll_media();
        self.poll_native();
        self.poll_control();
        self.update_floating_preview(ctx);
        while let Ok(e) = self.background_rx.try_recv() {
            match e {
                BackgroundEvent::Directory { screenshot, path } => {
                    if screenshot {
                        self.model.settings.screenshot_dir = path;
                    } else {
                        self.model.settings.recording_dir = path;
                    }
                }
                BackgroundEvent::Exported(p) => self.model.notify("诊断日志已导出", Some(p)),
                BackgroundEvent::Failed(e) => self.model.notify(e, None),
                BackgroundEvent::UiCaptured => {
                    if self.quit_after_capture {
                        self.request_transition(Transition::Exit);
                    }
                }
            }
        }
        if let Some(show) = self.tray.as_ref().and_then(|t| t.poll()) {
            if show {
                self.hidden = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else {
                self.request_transition(Transition::Exit);
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.exit_allowed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.process_action(Action::Close, ctx);
        }
        let foreground = platform::foreground(self.input_window())
            && (self.floating.is_some()
                || (!self.hidden && !ctx.input(|i| i.viewport().minimized.unwrap_or(false))));
        if self.was_foreground && !foreground {
            self.release("窗口失去焦点，键鼠已释放");
        }
        self.was_foreground = foreground;
        if ctx.input(|i| i.key_pressed(egui::Key::F9)) {
            self.release("已通过 F9 释放键鼠");
        }
        self.advance_transition(ctx);
        self.persist(false);
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.model.show(ui);
        if self.floating.is_none() && self.model.video_rect.is_none() && self.control.is_active() {
            self.release("投屏输入区域暂不可用，键鼠已释放");
        }
        let visible = self
            .model
            .video_rect
            .filter(|_| !self.hidden && self.model.pending.is_none());
        if self.floating.is_some() {
            self.update_floating_preview(&ctx);
        } else if let Some(rect) = visible {
            let scale = ctx.pixels_per_point();
            if let Some(preview) = &mut self.preview {
                if let Err(e) = preview.set_rect(
                    (rect.min.x * scale).round() as i32,
                    (rect.min.y * scale).round() as i32,
                    (rect.width() * scale).round() as i32,
                    (rect.height() * scale).round() as i32,
                ) {
                    tracing::error!(%e,"preview layout");
                }
                preview.set_visible(true);
                let radius = if self.model.settings.preview_rounded {
                    (self.model.settings.preview_corner_radius * scale).round() as u32
                } else {
                    0
                };
                if let Err(error) = preview.set_corner_radius(radius) {
                    tracing::error!(%error, "preview rounding");
                }
            }
            let [x, y, width, height] = platform::screen_rect(self.hwnd, rect, scale);
            self.control.send(ControlCommand::SetVideoRect {
                x,
                y,
                width,
                height,
            });
            if ctx.input(|i| {
                i.pointer.primary_clicked()
                    && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
            }) && self.video_focus
            {
                self.release("投屏区域失去输入焦点");
            }
        } else {
            if let Some(preview) = &mut self.preview {
                preview.set_visible(false);
            }
            self.control.send(ControlCommand::SetVideoRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
        }
        for action in std::mem::take(&mut self.model.actions) {
            self.process_action(action, &ctx);
        }
        if self.model.settings.remember_window
            && !ctx.input(|i| {
                i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
            })
            && let Some(r) = ctx.input(|i| i.viewport().inner_rect)
        {
            self.model.settings.window_size = [
                r.width() * ctx.zoom_factor(),
                r.height() * ctx.zoom_factor(),
            ];
        }
        self.capture_frame += 1;
        if self.capture_frame >= 10
            && !self.screenshot_requested
            && let Some(path) = self.screenshot_path.take()
        {
            self.screenshot_requested = true;
            let sender = self.background_tx.clone();
            ctx.request_screenshot(move |image| {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(
                    &path,
                    &rgba,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    tracing::error!(%e,"UI screenshot failed");
                }
                let _ = sender.send(BackgroundEvent::UiCaptured);
            });
        }
    }
    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        self.release("应用退出");
        self.dock_preview();
        self.recorder.stop();
        self.persist(true);
        self.command(Command::DetachPreview);
        self.command(Command::Shutdown);
    }
}

fn capture_options(settings: &Settings) -> native::CaptureOptions {
    let mut o = native::default_capture_options();
    o.requested_width = settings.preview_long_edge;
    o.requested_height = settings.preview_long_edge;
    o.target_fps = settings.preview_fps;
    o.play_audio = i32::from(settings.play_audio);
    o.audio_volume = settings.volume;
    o.reserved[2] = settings.usb_mode;
    o
}
