//! Native capture service. All discovery, receiver and lifecycle operations run
//! off the UI thread. Generation IDs belong to the application and are echoed
//! unchanged; neither discovery nor reconnect can replace a selected session.
mod ffi;
mod preview;
mod session;

pub use ffi::{CaptureOptions, VideoOutputStatus};
pub use preview::PreviewWindow;
pub use session::{AudioPacket, NativeSession, SessionStatus, VideoFrame};

use session::Core;
use std::{
    mem::size_of,
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionKind {
    AirPlay,
    Usb,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub kind: ConnectionKind,
    pub state: i32,
    pub usb_connected: bool,
    pub paired: bool,
    pub trusted: bool,
    pub product_type: String,
    pub os_version: String,
    pub message: String,
    /// Exact-serial read-only libusb0 check. None means not checked.
    pub driver_available: Option<bool>,
}
#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub apple_service_installed: bool,
    pub apple_service_running: bool,
    pub usbmux_available: bool,
    pub capture_usbmux_available: bool,
    pub physical_apple_devices: u32,
    pub libusb_available: bool,
    pub diagnostic: String,
}

#[derive(Debug)]
pub enum Command {
    RefreshDevices,
    Connect {
        device: Device,
        generation: u64,
    },
    /// Must match the current generation. UI must finish recording first.
    Disconnect {
        generation: u64,
    },
    AttachPreview {
        hwnd: usize,
    },
    DetachPreview,
    RefreshPreview,
    SetAudio {
        enabled: bool,
        volume: f32,
    },
    SetPreview {
        width: u32,
        height: u32,
        fps: u32,
    },
    SetUsbMode(u32),
    RestartReceiver {
        name: String,
    },
    Shutdown,
}
#[derive(Debug)]
pub enum Event {
    Ready {
        receiver_ready: bool,
    },
    Devices {
        usb: Vec<Device>,
        wireless: Vec<Device>,
        environment: Environment,
    },
    ReceiverStatus {
        running: bool,
        ready: bool,
    },
    SessionStarted {
        generation: u64,
        device: Device,
        session: Arc<NativeSession>,
    },
    SessionStatus {
        generation: u64,
        status: SessionStatus,
    },
    SessionStopped {
        generation: u64,
    },
    Error {
        generation: Option<u64>,
        message: String,
    },
    ShutdownComplete,
}

pub fn default_capture_options() -> CaptureOptions {
    CaptureOptions {
        requested_width: 1920,
        requested_height: 1080,
        target_fps: 60,
        play_audio: 1,
        audio_volume: 1.0,
        ..Default::default()
    }
}

pub struct NativeBackend {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
}
impl NativeBackend {
    pub fn spawn(runtime_dir: PathBuf, receiver_name: String, options: CaptureOptions) -> Self {
        let (commands, rx) = mpsc::channel();
        let (tx, events) = mpsc::channel();
        let thread_result = thread::Builder::new()
            .name("iphoneplay-capture".into())
            .spawn(move || {
                let result = Worker::new(runtime_dir, options, tx.clone());
                match result {
                    Ok(mut worker) => worker.run(receiver_name, rx),
                    Err(message) => {
                        let _ = tx.send(Event::Error {
                            generation: None,
                            message,
                        });
                    }
                }
            });
        // A failed OS thread creation still returns a usable error stream.
        if let Err(error) = thread_result {
            let (fallback_tx, fallback_rx) = mpsc::channel();
            let _ = fallback_tx.send(Event::Error {
                generation: None,
                message: format!("无法启动投屏服务线程：{error}"),
            });
            return Self {
                commands,
                events: fallback_rx,
            };
        }
        Self { commands, events }
    }
    pub fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "投屏服务已停止".into())
    }
    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
}
impl Drop for NativeBackend {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

struct Worker {
    core: Arc<Core>,
    runtime_dir: PathBuf,
    options: CaptureOptions,
    events: mpsc::Sender<Event>,
    session: Option<(u64, Arc<NativeSession>)>,
    preview: usize,
    last_receiver: Option<(bool, bool)>,
    next_devices: Instant,
    next_status: Instant,
}
impl Worker {
    fn new(
        runtime_dir: PathBuf,
        options: CaptureOptions,
        events: mpsc::Sender<Event>,
    ) -> Result<Self, String> {
        let core = Core::load(&runtime_dir.join("iPhoneMirror.Core.dll"))?;
        // Check a size-versioned call before Ready, so a bad structure layout
        // cannot masquerade as a usable receiver service.
        let mut environment = ffi::EnvironmentInfo::default();
        core.check(
            unsafe { (core.api.environment)(&mut environment) },
            "校验运行环境结构",
        )?;
        ffi::validate_header(
            environment.struct_size,
            environment.api_version,
            size_of::<ffi::EnvironmentInfo>(),
        )?;
        Ok(Self {
            core,
            runtime_dir,
            options,
            events,
            session: None,
            preview: 0,
            last_receiver: None,
            next_devices: Instant::now(),
            next_status: Instant::now(),
        })
    }
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
    fn report(&self, generation: Option<u64>, result: Result<(), String>) {
        if let Err(message) = result {
            self.emit(Event::Error {
                generation,
                message,
            });
        }
    }
    fn start_receiver(&self, name: &str) -> Result<(), String> {
        let host = self
            .runtime_dir
            .join("Wireless/iPhoneMirror.WirelessHost.exe");
        if !host.is_file() {
            return Err(format!("缺少 AirPlay 无线宿主：{}", host.display()));
        }
        let name = ffi::wide(name)?;
        let host = ffi::wide_path(&host)?;
        // Capture negotiation is deliberately independent of local preview caps.
        self.core.check(
            unsafe { (self.core.api.receiver_start)(name.as_ptr(), host.as_ptr(), 1920, 1080, 60) },
            "启动 AirPlay 接收服务",
        )
    }
    fn run(&mut self, receiver_name: String, commands: mpsc::Receiver<Command>) {
        self.report(None, self.start_receiver(&receiver_name));
        self.emit(Event::Ready {
            receiver_ready: false,
        });
        loop {
            match commands.recv_timeout(Duration::from_millis(50)) {
                Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(command) => self.command(command),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            if now >= self.next_status {
                self.poll_status();
                self.next_status = now + Duration::from_millis(250);
            }
            if now >= self.next_devices {
                self.refresh_devices(false);
                self.next_devices = Instant::now() + Duration::from_secs(3);
            }
        }
        if let Some((generation, session)) = self.session.take() {
            session.detach(self.preview);
            self.report(Some(generation), session.stop());
            drop(session);
            self.emit(Event::SessionStopped { generation });
        }
        unsafe {
            (self.core.api.receiver_stop)();
        }
        self.emit(Event::ShutdownComplete);
    }
    fn command(&mut self, command: Command) {
        match command {
            Command::RefreshDevices => self.refresh_devices(true),
            Command::Connect { device, generation } => {
                if self.session.is_some() {
                    self.emit(Event::Error {
                        generation: Some(generation),
                        message: "请先结束当前投屏会话，再连接其他设备".into(),
                    });
                    return;
                }
                let result = self.connect(&device);
                match result {
                    Ok(session) => {
                        if self.preview != 0 {
                            self.report(Some(generation), session.attach(self.preview));
                        }
                        self.session = Some((generation, session.clone()));
                        self.emit(Event::SessionStarted {
                            generation,
                            device,
                            session,
                        });
                    }
                    Err(message) => self.emit(Event::Error {
                        generation: Some(generation),
                        message,
                    }),
                }
            }
            Command::Disconnect { generation } => {
                if self
                    .session
                    .as_ref()
                    .is_some_and(|(current, _)| *current == generation)
                {
                    let (_, session) = self.session.take().expect("generation checked");
                    session.detach(self.preview);
                    self.report(Some(generation), session.stop());
                    drop(session);
                    self.emit(Event::SessionStopped { generation });
                }
            }
            Command::AttachPreview { hwnd } => {
                if hwnd != self.preview {
                    if let Some((generation, session)) = &self.session {
                        session.detach(self.preview);
                        self.report(Some(*generation), session.attach(hwnd));
                    }
                    self.preview = hwnd;
                }
            }
            Command::DetachPreview => {
                if let Some((_, session)) = &self.session {
                    session.detach(self.preview);
                }
                self.preview = 0;
            }
            Command::RefreshPreview => {
                if let Some((generation, session)) = &self.session {
                    self.report(Some(*generation), session.refresh_preview());
                }
            }
            Command::SetAudio { enabled, volume } => {
                self.options.play_audio = i32::from(enabled);
                self.options.audio_volume = volume.clamp(0.0, 1.0);
                if let Some((generation, session)) = &self.session {
                    self.report(Some(*generation), session.audio(enabled, volume));
                }
            }
            Command::SetPreview { width, height, fps } => {
                self.options.requested_width = width;
                self.options.requested_height = height;
                self.options.target_fps = fps;
                if let Some((generation, session)) = &self.session {
                    self.report(Some(*generation), session.preview(width, height, fps));
                }
            }
            Command::SetUsbMode(mode) => self.options.reserved[2] = mode.min(2),
            Command::RestartReceiver { name } => {
                if self.session.is_some() {
                    self.emit(Event::Error {
                        generation: None,
                        message: "请在结束当前投屏后更改接收名称".into(),
                    });
                } else {
                    unsafe {
                        (self.core.api.receiver_stop)();
                    }
                    self.last_receiver = None;
                    self.report(None, self.start_receiver(&name));
                }
            }
            Command::Shutdown => unreachable!("handled by worker loop"),
        }
    }
    fn connect(&self, device: &Device) -> Result<Arc<NativeSession>, String> {
        let id = ffi::wide(&device.id)?;
        if device.kind == ConnectionKind::Usb {
            let mut available = 0;
            self.core.check(
                unsafe { (self.core.api.usb_driver_available)(id.as_ptr(), &mut available) },
                "检查 USB 驱动",
            )?;
            if available == 0 {
                return Err(
                    "当前 iPhone 缺少可用的 USB 投屏过滤驱动，请打开驱动管理器完成安装后重新检测"
                        .into(),
                );
            }
        }
        let mut handle = 0;
        let create = match device.kind {
            ConnectionKind::AirPlay => self.core.api.wireless_create,
            ConnectionKind::Usb => self.core.api.usb_create,
        };
        self.core.check(
            unsafe { create(id.as_ptr(), &self.options, &mut handle) },
            "创建投屏会话",
        )?;
        if handle == 0 {
            return Err("投屏核心返回了无效的会话句柄".into());
        }
        Ok(Arc::new(NativeSession::new(self.core.clone(), handle)))
    }
    fn poll_status(&mut self) {
        let mut running = 0;
        let mut ready = 0;
        let rc = unsafe { (self.core.api.receiver_status)(&mut running, &mut ready) };
        if rc == 0 {
            let current = (running != 0, ready != 0);
            if self.last_receiver != Some(current) {
                self.last_receiver = Some(current);
                self.emit(Event::ReceiverStatus {
                    running: current.0,
                    ready: current.1,
                });
            }
        }
        if let Some((generation, session)) = &self.session {
            match session.status() {
                Ok(status) => self.emit(Event::SessionStatus {
                    generation: *generation,
                    status,
                }),
                Err(message) => self.emit(Event::Error {
                    generation: Some(*generation),
                    message,
                }),
            }
        }
    }
    fn refresh_devices(&self, metadata: bool) {
        let result = (|| {
            if metadata && self.session.is_none() {
                let mut count = 0;
                self.core.check(
                    unsafe { (self.core.api.refresh_usb_metadata)(ptr::null_mut(), &mut count, 1) },
                    "读取设备信任信息",
                )?;
            }
            let usb = self.enumerate(self.core.api.refresh_usb, ConnectionKind::Usb)?;
            let wireless =
                self.enumerate(self.core.api.refresh_wireless, ConnectionKind::AirPlay)?;
            let mut raw = ffi::EnvironmentInfo::default();
            self.core.check(
                unsafe { (self.core.api.environment)(&mut raw) },
                "检查运行环境",
            )?;
            ffi::validate_header(
                raw.struct_size,
                raw.api_version,
                size_of::<ffi::EnvironmentInfo>(),
            )?;
            Ok((
                usb,
                wireless,
                Environment {
                    apple_service_installed: raw.apple_mobile_device_service_installed != 0,
                    apple_service_running: raw.apple_mobile_device_service_running != 0,
                    usbmux_available: raw.standard_usbmux_available != 0,
                    capture_usbmux_available: raw.capture_usbmux_available != 0,
                    physical_apple_devices: raw.physical_apple_usb_devices,
                    libusb_available: raw.libusb_runtime_available != 0,
                    diagnostic: ffi::text(&raw.diagnostic),
                },
            ))
        })();
        match result {
            Ok((usb, wireless, environment)) => self.emit(Event::Devices {
                usb,
                wireless,
                environment,
            }),
            Err(message) => self.emit(Event::Error {
                generation: None,
                message,
            }),
        }
    }
    fn enumerate(
        &self,
        refresh: ffi::Refresh,
        kind: ConnectionKind,
    ) -> Result<Vec<Device>, String> {
        let mut count = 0;
        self.core
            .check(unsafe { refresh(ptr::null_mut(), &mut count) }, "扫描设备")?;
        if count == 0 {
            return Ok(vec![]);
        }
        for _ in 0..3 {
            if count > 256 {
                return Err("核心返回的设备数量异常".into());
            }
            let mut entries = vec![ffi::DeviceInfo::default(); count as usize];
            let capacity = count;
            let rc = unsafe { refresh(entries.as_mut_ptr(), &mut count) };
            if rc == ffi::BUFFER_TOO_SMALL {
                continue;
            }
            self.core.check(rc, "读取设备列表")?;
            if count > capacity {
                return Err("核心返回的设备数量超过缓冲区".into());
            }
            return entries
                .into_iter()
                .take(count as usize)
                .map(|raw| {
                    ffi::validate_header(
                        raw.struct_size,
                        raw.api_version,
                        size_of::<ffi::DeviceInfo>(),
                    )?;
                    let id = ffi::text(&raw.udid);
                    let driver_available = if kind == ConnectionKind::Usb && self.session.is_none()
                    {
                        let mut available = 0;
                        let udid = ffi::wide(&id)?;
                        let rc = unsafe {
                            (self.core.api.usb_driver_available)(udid.as_ptr(), &mut available)
                        };
                        if rc == 0 { Some(available != 0) } else { None }
                    } else {
                        None
                    };
                    Ok(Device {
                        id,
                        name: ffi::text(&raw.name),
                        kind,
                        state: raw.state,
                        usb_connected: raw.usb_connected != 0,
                        paired: raw.pair_record_present != 0,
                        trusted: raw.lockdown_accessible != 0,
                        product_type: ffi::text(&raw.product_type),
                        os_version: ffi::text(&raw.os_version),
                        message: ffi::text(&raw.status),
                        driver_available,
                    })
                })
                .collect();
        }
        Err("设备列表持续变化，请重新检测".into())
    }
}

/// Static dependency diagnostics; does not start receivers or modify drivers.
pub fn missing_dependencies(runtime_dir: &Path) -> Vec<PathBuf> {
    [
        "iPhoneMirror.Core.dll",
        "libusb-1.0.dll",
        "libusb0.dll",
        "Wireless/iPhoneMirror.WirelessHost.exe",
        "Wireless/airplay2dll.dll",
        "Wireless/avcodec-58.dll",
        "Wireless/avutil-56.dll",
        "Wireless/swresample-3.dll",
        "Wireless/swscale-5.dll",
        "Wireless/dnssd.dll",
    ]
    .iter()
    .map(|name| runtime_dir.join(name))
    .filter(|path| !path.is_file())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_runtime_is_reported_with_exact_paths() {
        let directory = tempfile::tempdir().unwrap();
        let missing = missing_dependencies(directory.path());
        assert_eq!(missing.len(), 10);
        assert!(missing.iter().any(|p| p.ends_with("iPhoneMirror.Core.dll")));
        assert!(
            missing
                .iter()
                .any(|p| p.ends_with("Wireless/airplay2dll.dll"))
        );
    }

    #[test]
    fn receiver_service_is_not_a_streaming_session() {
        let status = SessionStatus {
            state: 4,
            width: 1170,
            height: 2532,
            video_frames: 0,
            ..Default::default()
        };
        assert!(!status.has_video());
        assert!(
            SessionStatus {
                video_frames: 1,
                ..status
            }
            .has_video()
        );
    }

    #[test]
    fn missing_library_can_be_retried_without_stale_initialization() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("iPhoneMirror.Core.dll");
        for _ in 0..2 {
            let error = Core::load(&missing).err().expect("missing DLL must fail");
            assert!(error.contains("无法加载投屏核心"), "{error}");
        }
    }

    #[test]
    #[ignore = "requires built Windows runtime; starts a real AirPlay receiver, no phone required"]
    fn native_receiver_start_stop_smoke() {
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime");
        assert!(missing_dependencies(&runtime).is_empty());
        let backend = NativeBackend::spawn(
            runtime,
            "iPhonePlay · runtime test".into(),
            default_capture_options(),
        );
        let deadline = Instant::now() + Duration::from_secs(25);
        let mut ready = false;
        let mut errors = vec![];
        while Instant::now() < deadline && !ready {
            if let Some(event) = backend.try_recv() {
                match event {
                    Event::ReceiverStatus {
                        running: true,
                        ready: true,
                    } => ready = true,
                    Event::Error { message, .. } => errors.push(message),
                    _ => {}
                }
            } else {
                thread::sleep(Duration::from_millis(20));
            }
        }
        backend.send(Command::Shutdown).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut stopped = false;
        while Instant::now() < deadline && !stopped {
            stopped = matches!(backend.try_recv(), Some(Event::ShutdownComplete));
            if !stopped {
                thread::sleep(Duration::from_millis(20));
            }
        }
        assert!(stopped, "native shutdown did not complete");
        assert!(ready, "receiver never became ready: {errors:?}");
    }
}
