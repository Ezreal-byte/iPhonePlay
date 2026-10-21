//! A single-device BLE HID service. UI calls enqueue commands; all WinRT work
//! lives on the worker. The input gate can be closed synchronously from any thread.
mod hooks;
pub mod protocol;
mod transport;

use protocol::{KeyboardState, mouse_report};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControlState {
    #[default]
    Off,
    Checking,
    WaitingPairing,
    WaitingConfirmation,
    Ready,
    Active,
    Paused,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct ControlSnapshot {
    pub session_id: u64,
    pub state: ControlState,
    pub advertised_name: String,
    pub clients: Vec<ClientInfo>,
    pub bound_client: Option<String>,
    pub message: String,
}

impl ControlSnapshot {
    pub fn is_ready(&self) -> bool {
        self.session_id != 0
            && matches!(
                self.state,
                ControlState::Ready | ControlState::Paused | ControlState::Active
            )
            && self
                .bound_client
                .as_ref()
                .is_some_and(|id| self.clients.iter().any(|client| &client.id == id))
    }
}

#[derive(Clone, Debug)]
pub enum ControlEvent {
    Snapshot(ControlSnapshot),
    Error(String),
}

#[derive(Clone, Debug)]
pub enum ControlCommand {
    SetWindow(isize),
    SetVideoRect {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    Start {
        session_id: u64,
        device_id: String,
    },
    ConfirmClient(String),
    Activate,
    Pause(String),
    Focus {
        session_id: u64,
        focused: bool,
    },
    SetSensitivity {
        mouse: f32,
        wheel: f32,
    },
    ForgetBinding,
    Stop,
}

#[derive(Clone, Debug)]
pub(super) enum Input {
    Key {
        vk: u32,
        scan: u32,
        extended: bool,
        down: bool,
    },
    Button {
        mask: u8,
        down: bool,
    },
    Wheel(i32),
    Move {
        dx: i32,
        dy: i32,
    },
}

pub(super) enum Message {
    Command(u64, ControlCommand),
    Input(u64, Input),
    Release(String),
    Shutdown,
}

#[derive(Default)]
pub(super) struct PendingMotion {
    generation: u64,
    dx: i32,
    dy: i32,
}

pub(super) struct Gate {
    pub active: AtomicBool,
    pub focused: AtomicBool,
    pub target_valid: AtomicBool,
    pub generation: AtomicU64,
    pub window: AtomicIsize,
    pub session: AtomicU64,
    pub video_rect: Mutex<Option<[i32; 4]>>,
    pub shutdown: AtomicBool,
    transition: Mutex<()>,
    pub motion: Mutex<PendingMotion>,
    pub sender: Sender<Message>,
}

impl Gate {
    pub fn close(&self) {
        let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
        self.active.store(false, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut m) = self.motion.lock() {
            *m = PendingMotion::default();
        }
    }
    fn activate(&self, expected_generation: u64) -> bool {
        let _transition = self.transition.lock().unwrap_or_else(|e| e.into_inner());
        if expected_generation != self.generation.load(Ordering::Acquire)
            || !self.focused.load(Ordering::Acquire)
            || !self.target_valid.load(Ordering::Acquire)
            || self.shutdown.load(Ordering::Acquire)
        {
            return false;
        }
        self.active.store(true, Ordering::Release);
        true
    }
    pub fn release(&self, reason: &str) {
        self.close();
        let _ = self.sender.send(Message::Release(reason.to_owned()));
    }
    pub fn admits(&self) -> bool {
        self.active.load(Ordering::Acquire)
            && self.focused.load(Ordering::Acquire)
            && self.target_valid.load(Ordering::Acquire)
            && !self.shutdown.load(Ordering::Acquire)
    }
    pub fn motion(&self, dx: i32, dy: i32) {
        if !self.admits() {
            return;
        }
        let generation = self.generation.load(Ordering::Acquire);
        if let Ok(mut m) = self.motion.lock() {
            if m.generation != generation {
                *m = PendingMotion {
                    generation,
                    dx: 0,
                    dy: 0,
                };
            }
            m.dx = m.dx.saturating_add(dx);
            m.dy = m.dy.saturating_add(dy);
        }
    }
    fn take_motion(&self) -> Option<(u64, Input)> {
        let mut m = self.motion.lock().ok()?;
        if m.dx == 0 && m.dy == 0 {
            return None;
        }
        let result = (m.generation, Input::Move { dx: m.dx, dy: m.dy });
        *m = PendingMotion::default();
        Some(result)
    }
    pub fn edge(&self, input: Input) {
        if !self.admits() {
            return;
        }
        // Flush accumulated movement before button/key edges. Edge events use
        // the unbounded channel and are never silently dropped under load.
        if let Some((generation, motion)) = self.take_motion() {
            let _ = self.sender.send(Message::Input(generation, motion));
        }
        let _ = self.sender.send(Message::Input(
            self.generation.load(Ordering::Acquire),
            input,
        ));
    }
}

pub struct ControlBackend {
    sender: Sender<Message>,
    events: Receiver<ControlEvent>,
    gate: Arc<Gate>,
    worker: Option<JoinHandle<()>>,
}

impl Default for ControlBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlBackend {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let (event_sender, events) = mpsc::channel();
        let gate = Arc::new(Gate {
            active: AtomicBool::new(false),
            focused: AtomicBool::new(false),
            target_valid: AtomicBool::new(false),
            generation: AtomicU64::new(1),
            window: AtomicIsize::new(0),
            session: AtomicU64::new(0),
            video_rect: Mutex::new(None),
            shutdown: AtomicBool::new(false),
            transition: Mutex::new(()),
            motion: Mutex::new(PendingMotion::default()),
            sender: sender.clone(),
        });
        let worker_gate = Arc::clone(&gate);
        let worker = thread::Builder::new()
            .name("iphoneplay-bluetooth".into())
            .spawn(move || {
                Worker::new(worker_gate, event_sender).run(receiver);
            })
            .ok();
        Self {
            sender,
            events,
            gate,
            worker,
        }
    }
    pub fn send(&self, command: ControlCommand) {
        match &command {
            ControlCommand::SetWindow(hwnd) => self.gate.window.store(*hwnd, Ordering::Release),
            ControlCommand::SetVideoRect {
                x,
                y,
                width,
                height,
            } => {
                if let Ok(mut rect) = self.gate.video_rect.lock() {
                    *rect = if *width > 0 && *height > 0 {
                        Some([*x, *y, *width, *height])
                    } else {
                        None
                    };
                }
                if (*width <= 0 || *height <= 0) && self.gate.active.load(Ordering::Acquire) {
                    self.gate.release("投屏画面已隐藏，键鼠已释放。");
                }
            }
            ControlCommand::Focus {
                session_id,
                focused,
            } => {
                let admitted = *focused && *session_id == self.gate.session.load(Ordering::Acquire);
                self.gate.focused.store(admitted, Ordering::Release);
                if !admitted {
                    self.gate.close();
                }
            }
            ControlCommand::Start { session_id, .. } => {
                self.gate.close();
                self.gate.focused.store(false, Ordering::Release);
                self.gate.session.store(*session_id, Ordering::Release);
            }
            ControlCommand::Stop => {
                self.gate.close();
                self.gate.session.store(0, Ordering::Release);
                self.gate.focused.store(false, Ordering::Release);
                self.gate.target_valid.store(false, Ordering::Release);
            }
            ControlCommand::Pause(_) | ControlCommand::ForgetBinding => self.gate.close(),
            _ => {}
        }
        let _ = self.sender.send(Message::Command(
            self.gate.generation.load(Ordering::Acquire),
            command,
        ));
    }
    pub fn release(&self, reason: impl AsRef<str>) {
        self.gate.release(reason.as_ref());
    }
    pub fn try_event(&self) -> Option<ControlEvent> {
        self.events.try_recv().ok()
    }
    pub fn is_active(&self) -> bool {
        self.gate.admits()
    }
    /// Close the admission gate immediately; service/hook cleanup remains on
    /// the worker. Poll `shutdown_complete` instead of blocking the UI.
    pub fn request_shutdown(&self) {
        self.gate.close();
        self.gate.session.store(0, Ordering::Release);
        self.gate.focused.store(false, Ordering::Release);
        self.gate.target_valid.store(false, Ordering::Release);
        self.gate.shutdown.store(true, Ordering::Release);
        let _ = self.sender.send(Message::Shutdown);
    }
    /// Also acknowledges initialization failure or an already exited worker.
    pub fn shutdown_complete(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for ControlBackend {
    fn drop(&mut self) {
        self.request_shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct Route {
    session: u64,
    device: String,
    client: Option<String>,
}

impl Route {
    fn start(&mut self, session: u64, device: String) {
        // Reuse a confirmed association only for the same capture identity.
        if self.device != device {
            self.client = None;
        }
        self.device = device;
        self.session = session;
    }
    fn confirm(&mut self, id: String, clients: &[ClientInfo]) -> bool {
        if self.device.is_empty() || !clients.iter().any(|c| c.id == id) {
            return false;
        }
        self.client = Some(id);
        true
    }
    fn available(&self, clients: &[ClientInfo]) -> bool {
        self.client
            .as_ref()
            .is_some_and(|id| clients.iter().any(|c| &c.id == id))
    }
}

struct Worker {
    gate: Arc<Gate>,
    events: Sender<ControlEvent>,
    snapshot: ControlSnapshot,
    route: Route,
    transport: Option<transport::Transport>,
    hooks: Option<hooks::HookThread>,
    keyboard: KeyboardState,
    buttons: u8,
    mouse_sensitivity: f32,
    wheel_sensitivity: f32,
    wheel_remainder: f32,
    remote_input_dirty: bool,
}

impl Worker {
    fn new(gate: Arc<Gate>, events: Sender<ControlEvent>) -> Self {
        Self {
            gate,
            events,
            snapshot: ControlSnapshot {
                advertised_name: std::env::var("COMPUTERNAME")
                    .unwrap_or_else(|_| "Windows 电脑".into()),
                ..Default::default()
            },
            route: Route::default(),
            transport: None,
            hooks: None,
            keyboard: KeyboardState::default(),
            buttons: 0,
            mouse_sensitivity: 1.,
            wheel_sensitivity: 1.,
            wheel_remainder: 0.,
            remote_input_dirty: false,
        }
    }
    fn publish(&self) {
        let mut snapshot = self.snapshot.clone();
        snapshot.session_id = self.route.session;
        let _ = self.events.send(ControlEvent::Snapshot(snapshot));
    }
    fn state(&mut self, state: ControlState, message: impl Into<String>) {
        let previous = self.snapshot.state;
        self.snapshot.state = state;
        self.snapshot.message = message.into();
        tracing::info!(?previous, ?state, session = self.route.session,
            clients = self.snapshot.clients.len(), bound = self.route.client.is_some(),
            message = %self.snapshot.message, "BLE control state");
        self.publish();
    }
    fn run(mut self, receiver: Receiver<Message>) {
        if let Err(error) = transport::initialize() {
            self.state(
                ControlState::Failed,
                format!("无法初始化 Windows 蓝牙服务：{error}"),
            );
            return;
        }
        let mut last_refresh = Instant::now();
        while !self.gate.shutdown.load(Ordering::Acquire) {
            match receiver.recv_timeout(Duration::from_millis(8)) {
                Ok(Message::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Message::Command(generation, command)) => {
                    // User release/Stop can happen while a prior Activate is
                    // waiting behind WinRT work. Never reopen an expired gate.
                    if !matches!(
                        command,
                        ControlCommand::Activate | ControlCommand::ConfirmClient(_)
                    ) || generation == self.gate.generation.load(Ordering::Acquire)
                    {
                        self.command(command);
                    }
                }
                Ok(Message::Release(reason)) => {
                    self.pause_if_active(reason);
                }
                Ok(Message::Input(generation, input)) => self.input(generation, input),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some((generation, motion)) = self.gate.take_motion() {
                self.input(generation, motion);
            }
            if last_refresh.elapsed() >= Duration::from_millis(350) {
                self.refresh_clients();
                last_refresh = Instant::now();
            }
        }
        self.release_reports();
        self.hooks.take();
        self.transport.take();
        self.route.session = 0;
        self.snapshot.clients.clear();
        self.state(ControlState::Off, "蓝牙反控已停止");
        transport::uninitialize();
    }
    fn pause_if_active(&mut self, reason: String) {
        let was_active = self.snapshot.state == ControlState::Active;
        self.release_reports();
        if was_active {
            self.state(ControlState::Paused, reason);
        } else {
            // Opening Windows Bluetooth settings loses foreground focus while
            // pairing. Close the input gate, but keep that onboarding step and
            // its message: input has never been acquired at this point.
            tracing::debug!(state = ?self.snapshot.state, %reason,
                "BLE input gate closed without pausing onboarding");
        }
    }
    fn command(&mut self, command: ControlCommand) {
        match command {
            ControlCommand::SetWindow(_) | ControlCommand::SetVideoRect { .. } => {}
            ControlCommand::Start {
                session_id,
                device_id,
            } => {
                if session_id == 0 || session_id != self.gate.session.load(Ordering::Acquire) {
                    return;
                }
                self.release_reports();
                self.route.start(session_id, device_id);
                self.snapshot.bound_client = self.route.client.clone();
                self.state(ControlState::Checking, "正在创建蓝牙键鼠服务…");
                if self.transport.is_none() {
                    let gate = Arc::clone(&self.gate);
                    match transport::Transport::start(|| {
                        gate.shutdown.load(Ordering::Acquire)
                            || gate.session.load(Ordering::Acquire) != session_id
                    }) {
                        Ok(transport) => self.transport = Some(transport),
                        Err(error) => {
                            if self.gate.shutdown.load(Ordering::Acquire)
                                || self.gate.session.load(Ordering::Acquire) != session_id
                            {
                                return;
                            }
                            self.state(
                                ControlState::Failed,
                                format!(
                                    "蓝牙服务启动失败：{}。请重新尝试开启反控。",
                                    error.trim_end_matches('。')
                                ),
                            );
                            return;
                        }
                    }
                }
                if session_id != self.gate.session.load(Ordering::Acquire) {
                    self.transport.take();
                    return;
                }
                self.state(
                    ControlState::WaitingPairing,
                    "蓝牙键鼠服务已开启。请在 iPhone「设置 → 蓝牙」点击这台电脑；手机连接本应用的键鼠服务后会出现在下方。仅有 Windows 配对记录还不能控制。",
                );
                self.refresh_clients();
            }
            ControlCommand::ConfirmClient(client) => {
                self.release_reports();
                self.refresh_clients();
                if self.route.confirm(client.clone(), &self.snapshot.clients) {
                    self.snapshot.bound_client = Some(client);
                    self.gate.target_valid.store(true, Ordering::Release);
                    self.state(
                        ControlState::Ready,
                        "已确认当前手机。点击投屏画面后开启控制；F9 随时释放。",
                    );
                } else {
                    self.state(
                        ControlState::WaitingConfirmation,
                        "设备已离线，请重新选择仍连接的手机。",
                    );
                }
            }
            ControlCommand::Activate => {
                let activation_generation = self.gate.generation.load(Ordering::Acquire);
                if !self.route.available(&self.snapshot.clients)
                    || !self.gate.focused.load(Ordering::Acquire)
                    || self.route.session == 0
                {
                    self.state(
                        ControlState::Paused,
                        "请先确认蓝牙设备，并点击投屏区域再开启控制。",
                    );
                    return;
                }
                if !hooks::foreground_matches(self.gate.window.load(Ordering::Acquire))
                    || self.gate.video_rect.lock().is_ok_and(|rect| rect.is_none())
                {
                    self.state(ControlState::Paused, "窗口没有输入焦点，键鼠保持在本机。");
                    return;
                }
                if self.hooks.is_none() {
                    match hooks::HookThread::start(Arc::clone(&self.gate)) {
                        Ok(hooks) => self.hooks = Some(hooks),
                        Err(error) => {
                            self.state(ControlState::Failed, format!("无法启用键鼠输入：{error}"));
                            return;
                        }
                    }
                }
                if !self.gate.activate(activation_generation) {
                    return;
                }
                self.state(ControlState::Active, "正在控制 iPhone · 按 F9 释放键鼠");
            }
            ControlCommand::Pause(reason) => {
                self.pause_if_active(reason);
            }
            ControlCommand::Focus {
                session_id,
                focused,
            } => {
                if session_id != self.route.session || !focused {
                    self.gate.focused.store(false, Ordering::Release);
                    if self.snapshot.state == ControlState::Active {
                        self.release_reports();
                        self.state(
                            ControlState::Paused,
                            "已释放键鼠。返回画面后需手动重新开启控制。",
                        );
                    }
                }
            }
            ControlCommand::SetSensitivity { mouse, wheel } => {
                self.mouse_sensitivity = if mouse.is_finite() {
                    mouse.clamp(0.1, 5.)
                } else {
                    1.
                };
                self.wheel_sensitivity = if wheel.is_finite() {
                    wheel.clamp(0.1, 5.)
                } else {
                    1.
                };
            }
            ControlCommand::ForgetBinding => {
                self.release_reports();
                self.route.client = None;
                self.snapshot.bound_client = None;
                self.state(
                    ControlState::WaitingConfirmation,
                    "已解除应用内绑定；Windows 蓝牙配对记录保持不变。",
                );
            }
            ControlCommand::Stop => {
                self.release_reports();
                self.hooks.take();
                self.transport.take();
                self.route.session = 0;
                self.snapshot.clients.clear();
                self.state(ControlState::Off, "反控未启用");
            }
        }
    }
    fn refresh_clients(&mut self) {
        let Some(transport) = &self.transport else {
            return;
        };
        let clients = match transport.clients() {
            Ok(clients) => clients,
            Err(error) => {
                self.release_reports();
                self.transport.take();
                self.gate.target_valid.store(false, Ordering::Release);
                self.snapshot.clients.clear();
                self.state(ControlState::Failed, format!("蓝牙连接检查失败：{error}"));
                return;
            }
        };
        self.update_clients(clients);
    }
    fn update_clients(&mut self, clients: Vec<ClientInfo>) {
        let changed = clients != self.snapshot.clients;
        self.snapshot.clients = clients;
        let valid = self.route.available(&self.snapshot.clients);
        self.gate.target_valid.store(valid, Ordering::Release);
        if !valid && self.snapshot.state == ControlState::Active {
            self.release_reports();
            self.state(
                ControlState::Paused,
                "蓝牙已断开；投屏和录制继续。重新连接后请手动开启控制。",
            );
        } else if !valid {
            let state = if self.snapshot.clients.is_empty() {
                ControlState::WaitingPairing
            } else {
                ControlState::WaitingConfirmation
            };
            if changed || self.snapshot.state != state {
                self.state(state, if state == ControlState::WaitingPairing {
                    "等待 iPhone 连接键鼠服务。请在手机「设置 → 蓝牙」点击这台电脑；Windows 显示“已配对”还不代表键鼠已连接。"
                } else { "已检测到手机的键鼠连接。请选择并确认当前投屏的 iPhone；不会自动绑定。" });
            }
        } else if matches!(
            self.snapshot.state,
            ControlState::Checking
                | ControlState::WaitingPairing
                | ControlState::WaitingConfirmation
        ) {
            self.state(
                ControlState::Ready,
                "已确认的手机已连接键鼠服务。点击“开始控制”后才会接管输入；F9 随时释放。",
            );
        } else if changed {
            self.publish();
        }
    }
    fn input(&mut self, generation: u64, input: Input) {
        if generation != self.gate.generation.load(Ordering::Acquire) || !self.gate.admits() {
            return;
        }
        let (id, report) = match input {
            Input::Key {
                vk,
                scan,
                extended,
                down,
            } => {
                if let Some(usage) = consumer_usage(vk) {
                    (4, (if down { usage } else { 0_u16 }).to_le_bytes().to_vec())
                } else {
                    if !self.keyboard.update(vk, scan, extended, down) {
                        return;
                    }
                    (1, self.keyboard.report().to_vec())
                }
            }
            Input::Button { mask, down } => {
                if down {
                    self.buttons |= mask;
                } else {
                    self.buttons &= !mask;
                }
                (2, mouse_report(self.buttons, 0, 0, 0).to_vec())
            }
            Input::Wheel(delta) => {
                self.wheel_remainder += delta as f32 / 120. * self.wheel_sensitivity;
                let ticks = self.wheel_remainder.trunc() as i32;
                self.wheel_remainder -= ticks as f32;
                if ticks == 0 {
                    return;
                }
                (2, mouse_report(self.buttons, 0, 0, ticks).to_vec())
            }
            Input::Move { dx, dy } => (
                2,
                mouse_report(
                    self.buttons,
                    (dx as f32 * self.mouse_sensitivity).round() as i32,
                    (dy as f32 * self.mouse_sensitivity).round() as i32,
                    0,
                )
                .to_vec(),
            ),
        };
        if let (Some(transport), Some(client)) = (&self.transport, &self.route.client) {
            // Only real input makes a remote release necessary. Sending zero
            // probes while iOS is still subscribing can fail and tear down a
            // healthy new connection (also documented by the upstream).
            self.remote_input_dirty = true;
            if let Err(error) =
                transport.send_if(client, id, &report, Duration::from_millis(300), &|| {
                    generation == self.gate.generation.load(Ordering::Acquire)
                        && self.gate.admits()
                        && hooks::foreground_matches(self.gate.window.load(Ordering::Acquire))
                })
            {
                self.gate.close();
                self.keyboard.clear();
                self.buttons = 0;
                // Retiring the service prevents delayed input from being sent
                // to a future session after a timed-out WinRT notification.
                self.transport.take();
                self.hooks.take();
                self.gate.target_valid.store(false, Ordering::Release);
                let message = format!("蓝牙输入失败，已释放本机键鼠：{error}。请重新开启反控。");
                let _ = self.events.send(ControlEvent::Error(message.clone()));
                self.state(ControlState::Failed, message);
            }
        }
    }
    fn release_reports(&mut self) {
        let release_needed = std::mem::take(&mut self.remote_input_dirty);
        self.gate.close();
        self.keyboard.clear();
        self.buttons = 0;
        self.wheel_remainder = 0.;
        // Zero-state notifications are targeted at the previously confirmed
        // client only. No fallback to another subscribed phone is allowed.
        let mut release_failed = false;
        if release_needed
            && let (Some(transport), Some(client)) = (&self.transport, &self.route.client)
        {
            for (id, value) in [
                (1, vec![0; 8]),
                (2, vec![0; 6]),
                (4, vec![0; 2]),
                (5, vec![0; 2]),
            ] {
                if transport
                    .send(client, id, &value, Duration::from_millis(120))
                    .is_err()
                {
                    release_failed = true;
                    break;
                }
            }
        }
        if release_failed {
            // A stuck connection must not retain key state merely because a
            // best-effort release notification failed. Close the HID link.
            self.transport.take();
            self.gate.target_valid.store(false, Ordering::Release);
            self.snapshot.clients.clear();
            let _ = self.events.send(ControlEvent::Error(
                "蓝牙释放报告未获确认，已关闭连接以清除按键状态。请重新开启反控。".into(),
            ));
        }
    }
}

fn consumer_usage(vk: u32) -> Option<u16> {
    match vk {
        0xAD => Some(0xE2),
        0xAE => Some(0xEA),
        0xAF => Some(0xE9),
        0xB0 => Some(0xB5),
        0xB1 => Some(0xB6),
        0xB2 => Some(0xB7),
        0xB3 => Some(0xCD),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open_gate(sender: Sender<Message>) -> Gate {
        Gate {
            active: AtomicBool::new(true),
            focused: AtomicBool::new(true),
            target_valid: AtomicBool::new(true),
            generation: AtomicU64::new(4),
            window: AtomicIsize::new(1),
            session: AtomicU64::new(1),
            video_rect: Mutex::new(None),
            shutdown: AtomicBool::new(false),
            transition: Mutex::new(()),
            motion: Mutex::new(PendingMotion::default()),
            sender,
        }
    }
    fn client(id: &str) -> ClientInfo {
        ClientInfo {
            id: id.into(),
            name: "同名 iPhone".into(),
        }
    }
    fn test_worker() -> (Worker, Receiver<ControlEvent>) {
        let (sender, _) = mpsc::channel();
        let (events, receiver) = mpsc::channel();
        (Worker::new(Arc::new(open_gate(sender)), events), receiver)
    }
    #[test]
    fn focus_release_preserves_onboarding_and_non_active_messages() {
        for state in [
            ControlState::Off,
            ControlState::Checking,
            ControlState::WaitingPairing,
            ControlState::WaitingConfirmation,
            ControlState::Ready,
            ControlState::Paused,
            ControlState::Failed,
        ] {
            let (mut worker, events) = test_worker();
            worker.snapshot.state = state;
            worker.snapshot.message = "原来的步骤或错误说明".into();
            worker.gate.motion(12, 34);
            worker.keyboard.update(0x41, 0, false, true);
            worker.buttons = 1;
            worker.pause_if_active("窗口失去焦点，键鼠已释放".into());
            assert_eq!(worker.snapshot.state, state);
            assert_eq!(worker.snapshot.message, "原来的步骤或错误说明");
            assert!(!worker.gate.admits());
            assert!(worker.gate.take_motion().is_none());
            assert_eq!(worker.keyboard.report(), [0; 8]);
            assert_eq!(worker.buttons, 0);
            assert!(!worker.remote_input_dirty);
            assert!(events.try_recv().is_err(), "no misleading paused snapshot");
        }
    }
    #[test]
    fn active_focus_release_pauses_and_requires_explicit_reactivation() {
        let (mut worker, events) = test_worker();
        worker.snapshot.state = ControlState::Active;
        worker.remote_input_dirty = true;
        worker.pause_if_active("窗口失去焦点，键鼠已释放".into());
        assert_eq!(worker.snapshot.state, ControlState::Paused);
        assert_eq!(worker.snapshot.message, "窗口失去焦点，键鼠已释放");
        assert!(!worker.remote_input_dirty);
        assert!(matches!(events.try_recv(), Ok(ControlEvent::Snapshot(_))));
        worker.gate.focused.store(true, Ordering::Release);
        assert!(!worker.gate.admits());
    }
    #[test]
    fn explicit_pause_while_pairing_keeps_the_connection_instructions() {
        let (mut worker, _) = test_worker();
        worker.snapshot.state = ControlState::WaitingPairing;
        worker.snapshot.message = "在手机蓝牙设置连接这台电脑".into();
        worker.command(ControlCommand::Pause("进入设置，键鼠已释放".into()));
        assert_eq!(worker.snapshot.state, ControlState::WaitingPairing);
        assert_eq!(worker.snapshot.message, "在手机蓝牙设置连接这台电脑");
        assert!(!worker.gate.admits());
    }
    #[test]
    fn newly_subscribed_phone_is_shown_for_confirmation_without_auto_binding() {
        let (mut worker, _) = test_worker();
        worker.route.start(9, "phone-a".into());
        worker.snapshot.state = ControlState::WaitingPairing;
        worker.gate.close();
        worker.update_clients(vec![client("subscriber")]);
        assert_eq!(worker.snapshot.state, ControlState::WaitingConfirmation);
        assert_eq!(worker.snapshot.clients, vec![client("subscriber")]);
        assert!(worker.route.client.is_none());
        assert!(!worker.gate.target_valid.load(Ordering::Acquire));
        assert!(!worker.gate.admits());
        worker.pause_if_active("普通失焦".into());
        assert_eq!(worker.snapshot.state, ControlState::WaitingConfirmation);
    }
    #[test]
    fn disconnected_binding_returns_to_pairing_and_does_not_select_another_phone() {
        let (mut worker, _) = test_worker();
        worker.route.start(9, "phone-a".into());
        worker
            .route
            .confirm("confirmed".into(), &[client("confirmed")]);
        worker.snapshot.state = ControlState::Paused;
        worker.gate.close();
        worker.update_clients(vec![]);
        assert_eq!(worker.snapshot.state, ControlState::WaitingPairing);
        worker.update_clients(vec![client("another")]);
        assert_eq!(worker.snapshot.state, ControlState::WaitingConfirmation);
        assert_eq!(worker.route.client.as_deref(), Some("confirmed"));
        assert!(!worker.gate.target_valid.load(Ordering::Acquire));
        worker.update_clients(vec![client("confirmed")]);
        assert_eq!(worker.snapshot.state, ControlState::Ready);
        assert!(
            !worker.gate.admits(),
            "a reconnection never takes over input"
        );
    }
    #[test]
    fn no_automatic_client_selection_even_when_only_one_client_exists() {
        let mut route = Route::default();
        route.start(1, "phone-a".into());
        assert!(!route.available(&[client("one")]));
        assert!(route.confirm("one".into(), &[client("one")]));
        assert!(route.available(&[client("one")]));
        assert!(!route.available(&[client("two")]));
        assert_eq!(route.client.as_deref(), Some("one"));
    }
    #[test]
    fn changing_mirror_identity_clears_binding() {
        let mut route = Route::default();
        route.start(1, "phone-a".into());
        route.confirm("one".into(), &[client("one")]);
        route.start(2, "phone-b".into());
        assert!(route.client.is_none());
    }
    #[test]
    fn paused_snapshot_is_not_ready_without_the_confirmed_client() {
        let mut snapshot = ControlSnapshot {
            session_id: 9,
            state: ControlState::Paused,
            clients: vec![client("other-phone")],
            bound_client: Some("confirmed-phone".into()),
            ..Default::default()
        };
        assert!(!snapshot.is_ready());
        snapshot.clients.push(client("confirmed-phone"));
        assert!(snapshot.is_ready());
        snapshot.state = ControlState::Failed;
        assert!(!snapshot.is_ready());
    }
    #[test]
    fn shutdown_of_an_already_exited_worker_is_acknowledged() {
        let (sender, _) = mpsc::channel();
        let (_, events) = mpsc::channel();
        let gate = Arc::new(open_gate(sender.clone()));
        let worker = thread::spawn(|| {});
        while !worker.is_finished() {
            thread::yield_now();
        }
        let backend = ControlBackend {
            sender,
            events,
            gate,
            worker: Some(worker),
        };
        backend.request_shutdown();
        assert!(backend.shutdown_complete());
        assert!(!backend.is_active());
    }
    #[test]
    fn release_invalidates_queued_input_and_focus_does_not_reactivate() {
        let (sender, _) = mpsc::channel();
        let gate = open_gate(sender);
        gate.motion(50, 60);
        gate.close();
        assert_eq!(gate.generation.load(Ordering::Acquire), 5);
        assert!(gate.take_motion().is_none());
        gate.focused.store(true, Ordering::Release);
        assert!(!gate.admits());
        assert!(
            !gate.activate(4),
            "a queued pre-release Activate must not reopen control"
        );
        assert!(
            gate.activate(5),
            "only a fresh explicit Activate can reopen control"
        );
    }
    #[test]
    fn high_frequency_moves_are_coalesced_before_lossless_edges() {
        let (sender, receiver) = mpsc::channel();
        let gate = open_gate(sender);
        for _ in 0..1000 {
            gate.motion(1, -1);
        }
        for _ in 0..1000 {
            gate.edge(Input::Button {
                mask: 1,
                down: true,
            });
            gate.edge(Input::Button {
                mask: 1,
                down: false,
            });
        }
        let queued: Vec<_> = receiver.try_iter().collect();
        assert_eq!(queued.len(), 2001);
        assert!(matches!(
            queued[0],
            Message::Input(
                4,
                Input::Move {
                    dx: 1000,
                    dy: -1000
                }
            )
        ));
        for pair in queued[1..].chunks(2) {
            assert!(matches!(
                pair[0],
                Message::Input(
                    4,
                    Input::Button {
                        mask: 1,
                        down: true
                    }
                )
            ));
            assert!(matches!(
                pair[1],
                Message::Input(
                    4,
                    Input::Button {
                        mask: 1,
                        down: false
                    }
                )
            ));
        }
    }
}
