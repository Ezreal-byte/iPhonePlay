use super::{
    ClientInfo,
    protocol::{HID_INFORMATION, REPORT_MAP, boot_mouse_report},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use windows::{
    Devices::{
        Bluetooth::{BluetoothError, BluetoothLEDevice, GenericAttributeProfile::*},
        Enumeration::DeviceInformation,
    },
    Foundation::TypedEventHandler,
    Security::Cryptography::CryptographicBuffer,
    Storage::Streams::IBuffer,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::{GUID, HSTRING},
};

type Result<T> = std::result::Result<T, String>;

// Poll async WinRT operations only on worker threads. Canceling a notification
// is not proof it has stopped; the caller retires this transport on timeout.
macro_rules! wait {
    ($operation:expr, $timeout:expr) => {{
        let operation = $operation.map_err(|e| e.to_string())?;
        let deadline = Instant::now() + $timeout;
        loop {
            let status = operation.Status().map_err(|e| e.to_string())?;
            if status.0 != 0 {
                break operation.GetResults().map_err(|e| e.to_string());
            }
            if Instant::now() >= deadline {
                let _ = operation.Cancel();
                break Err("Windows 蓝牙操作超时".to_owned());
            }
            thread::sleep(Duration::from_millis(2));
        }
    }};
}

pub(super) fn initialize() -> Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())
}
pub(super) fn uninitialize() {
    unsafe {
        RoUninitialize();
    }
}

fn uuid(short: u16) -> GUID {
    GUID::from_u128(((short as u128) << 96) | 0x0000_1000_8000_0080_5f9b_34fb)
}
fn buffer(value: &[u8]) -> Result<IBuffer> {
    CryptographicBuffer::CreateFromByteArray(value).map_err(|e| e.to_string())
}

#[derive(Default, Clone)]
struct ClientState {
    protocol_mode: u8,
    wheel_multiplier: u8,
}
#[derive(Default)]
struct Values {
    clients: HashMap<String, ClientState>,
    reports: HashMap<u8, Vec<u8>>,
    read_requests: u64,
    write_requests: u64,
    subscription_changes: u64,
}
#[derive(Clone)]
enum ValueKind {
    Static(Vec<u8>),
    ProtocolMode,
    WheelMultiplier,
    Report(u8),
    BootMouse,
}

impl ValueKind {
    fn read_value(&self, values: &Values, client: &str) -> Vec<u8> {
        match self {
            Self::Static(value) => value.clone(),
            Self::ProtocolMode => vec![values.clients.get(client).map_or(1, |c| c.protocol_mode)],
            Self::WheelMultiplier => {
                vec![values.clients.get(client).map_or(1, |c| c.wheel_multiplier)]
            }
            Self::Report(id) => values.reports.get(id).cloned().unwrap_or_else(|| {
                vec![
                    0;
                    match id {
                        1 => 8,
                        2 => 6,
                        _ => 2,
                    }
                ]
            }),
            Self::BootMouse => {
                boot_mouse_report(values.reports.get(&2).map(Vec::as_slice).unwrap_or(&[0; 6]))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartupDecision {
    Waiting,
    Advertising,
    Connected,
    Failed { status: i32, error: i32 },
}

fn is_advertising(status: GattServiceProviderAdvertisementStatus) -> bool {
    status == GattServiceProviderAdvertisementStatus::Started
        || status == GattServiceProviderAdvertisementStatus::StartedWithoutAllAdvertisementData
}

/// A Windows pairing record is not an active HID connection. The count here
/// includes only subscribed clients whose actual GATT session is Active.
fn startup_decision(
    status: GattServiceProviderAdvertisementStatus,
    error: i32,
    active_subscriptions: usize,
    deadline_reached: bool,
) -> StartupDecision {
    if is_advertising(status) {
        StartupDecision::Advertising
    } else if active_subscriptions > 0 {
        StartupDecision::Connected
    } else if deadline_reached {
        StartupDecision::Failed {
            status: status.0,
            error,
        }
    } else {
        // Upstream explicitly documents MediaTek/Windows reporting
        // Aborted/Success while replacing an advertisement before Started.
        // Stopping the provider here aborts a valid startup ourselves.
        StartupDecision::Waiting
    }
}

#[derive(Clone, Copy, Debug)]
struct AdvertisementEvent {
    status: i32,
    error: i32,
}

struct Characteristic {
    value: GattLocalCharacteristic,
    read_token: Option<i64>,
    write_token: Option<i64>,
    subscribed_token: Option<i64>,
}
impl Drop for Characteristic {
    fn drop(&mut self) {
        if let Some(token) = self.read_token {
            let _ = self.value.RemoveReadRequested(token);
        }
        if let Some(token) = self.write_token {
            let _ = self.value.RemoveWriteRequested(token);
        }
        if let Some(token) = self.subscribed_token {
            let _ = self.value.RemoveSubscribedClientsChanged(token);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SubscriptionDiagnostic {
    counts: [u32; 6],
    inactive_sessions: u32,
    active_clients: usize,
    read_requests: u64,
    write_requests: u64,
    subscription_changes: u64,
}

pub(super) struct Transport {
    provider: GattServiceProvider,
    advertisement_token: i64,
    advertisement_events: Arc<Mutex<Vec<AdvertisementEvent>>>,
    unavailable_since: Mutex<Option<Instant>>,
    characteristics: Vec<Characteristic>,
    reports: HashMap<u8, GattLocalCharacteristic>,
    boot_keyboard: GattLocalCharacteristic,
    boot_mouse: GattLocalCharacteristic,
    values: Arc<Mutex<Values>>,
    names: Mutex<HashMap<String, String>>,
    last_subscription_diagnostic: Mutex<Option<SubscriptionDiagnostic>>,
}

impl Transport {
    pub fn start(cancelled: impl Fn() -> bool) -> Result<Self> {
        // Actual service creation/advertisement is the authority. Some working
        // adapters falsely report IsPeripheralRoleSupported == false.
        let result = wait!(
            GattServiceProvider::CreateAsync(uuid(0x1812)),
            Duration::from_secs(5)
        )?;
        let error = result.Error().map_err(|e| e.to_string())?;
        if error != BluetoothError::Success {
            return Err(format!("创建 HID 服务失败（{}）", error.0));
        }
        let provider = result.ServiceProvider().map_err(|e| e.to_string())?;
        let advertisement_events = Arc::new(Mutex::new(Vec::new()));
        let event_sink = Arc::clone(&advertisement_events);
        let advertisement_token = provider
            .AdvertisementStatusChanged(&TypedEventHandler::<
                GattServiceProvider,
                GattServiceProviderAdvertisementStatusChangedEventArgs,
            >::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    let event = AdvertisementEvent {
                        status: args.Status()?.0,
                        error: args.Error()?.0,
                    };
                    // Do not query GATT objects from this WinRT callback.
                    tracing::debug!(
                        status = event.status,
                        error = event.error,
                        "BLE advertisement state event"
                    );
                    if let Ok(mut events) = event_sink.lock() {
                        if events.len() == 16 {
                            events.remove(0);
                        }
                        events.push(event);
                    }
                }
                Ok(())
            }))
            .map_err(|e| e.to_string())?;
        let service = provider.Service().map_err(|e| e.to_string())?;
        let values = Arc::new(Mutex::new(Values::default()));
        let make = |short, properties, initial: &[u8], kind, reference| {
            if cancelled() {
                return Err("蓝牙服务启动已取消".to_owned());
            }
            create(
                &service, short, properties, initial, kind, &values, reference,
            )
        };
        let mut characteristics = Vec::new();
        let mut reports = HashMap::new();
        let read = GattCharacteristicProperties::Read;
        let notify = read | GattCharacteristicProperties::Notify;
        let write = GattCharacteristicProperties::Write
            | GattCharacteristicProperties::WriteWithoutResponse;
        characteristics.push(make(
            0x2A4E,
            read | GattCharacteristicProperties::Write,
            &[1],
            ValueKind::ProtocolMode,
            None,
        )?);
        characteristics.push(make(
            0x2A4A,
            read,
            HID_INFORMATION,
            ValueKind::Static(HID_INFORMATION.to_vec()),
            None,
        )?);
        characteristics.push(make(
            0x2A4B,
            read,
            REPORT_MAP,
            ValueKind::Static(REPORT_MAP.to_vec()),
            None,
        )?);
        characteristics.push(make(
            0x2A4C,
            GattCharacteristicProperties::WriteWithoutResponse,
            &[0],
            ValueKind::Static(vec![0]),
            None,
        )?);
        characteristics.push(make(
            0x2A4D,
            read | write,
            &[1],
            ValueKind::WheelMultiplier,
            Some((3, 2)),
        )?);
        for id in [1, 2, 4, 5] {
            let initial = vec![
                0;
                match id {
                    1 => 8,
                    2 => 6,
                    _ => 2,
                }
            ];
            let characteristic = make(
                0x2A4D,
                notify,
                &initial,
                ValueKind::Report(id),
                Some((id, 1)),
            )?;
            reports.insert(id, characteristic.value.clone());
            characteristics.push(characteristic);
        }
        let boot_keyboard = make(0x2A22, notify, &[0; 8], ValueKind::Report(1), None)?;
        let boot_mouse = make(0x2A33, notify, &[0; 3], ValueKind::BootMouse, None)?;
        let keyboard_value = boot_keyboard.value.clone();
        let mouse_value = boot_mouse.value.clone();
        characteristics.push(boot_keyboard);
        characteristics.push(boot_mouse);
        let parameters =
            GattServiceProviderAdvertisingParameters::new().map_err(|e| e.to_string())?;
        parameters
            .SetIsConnectable(true)
            .map_err(|e| e.to_string())?;
        parameters
            .SetIsDiscoverable(true)
            .map_err(|e| e.to_string())?;
        let transport = Self {
            provider,
            advertisement_token,
            advertisement_events,
            unavailable_since: Mutex::new(None),
            characteristics,
            reports,
            boot_keyboard: keyboard_value,
            boot_mouse: mouse_value,
            values,
            names: Mutex::new(HashMap::new()),
            last_subscription_diagnostic: Mutex::new(None),
        };
        transport
            .provider
            .StartAdvertisingWithParameters(&parameters)
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut next_subscription_probe = Instant::now();
        let mut active_subscriptions = 0;
        loop {
            if cancelled() {
                return Err("蓝牙服务启动已取消".into());
            }
            let status = transport
                .provider
                .AdvertisementStatus()
                .map_err(|e| e.to_string())?;
            let now = Instant::now();
            if !is_advertising(status) && (now >= next_subscription_probe || now >= deadline) {
                active_subscriptions = transport.active_client_ids()?.len();
                next_subscription_probe = now + Duration::from_millis(100);
            }
            let error = transport.last_advertisement_error(status);
            match startup_decision(status, error, active_subscriptions, now >= deadline) {
                StartupDecision::Advertising => break,
                StartupDecision::Connected => {
                    tracing::info!(
                        status = status.0,
                        active_subscriptions,
                        "BLE HID connection active while advertisement is not running"
                    );
                    break;
                }
                StartupDecision::Failed { status, error } => {
                    return Err(format!(
                        "蓝牙服务在 5 秒内未完成启动（广播状态 {status}，{}，活跃 HID 订阅 0；状态事件 {}）。已配对不代表已连接本应用的键鼠服务。",
                        bluetooth_error(error),
                        transport.advertisement_trace()
                    ));
                }
                StartupDecision::Waiting => {}
            }
            thread::sleep(Duration::from_millis(20));
        }
        tracing::info!(events = %transport.advertisement_trace(), "BLE HID startup completed");
        Ok(transport)
    }

    fn active_client_ids(&self) -> Result<Vec<String>> {
        let mut clients = BTreeMap::new();
        let mut counts = [0; 6];
        let mut inactive_sessions = 0;
        // Match upstream's six input characteristics. Pairing records are not
        // candidates; only this provider's real notification subscriptions are.
        for (index, characteristic) in [
            &self.reports[&2],
            &self.reports[&1],
            &self.reports[&4],
            &self.reports[&5],
            &self.boot_mouse,
            &self.boot_keyboard,
        ]
        .into_iter()
        .enumerate()
        {
            let subscribed = characteristic
                .SubscribedClients()
                .map_err(|e| e.to_string())?;
            for client in subscribed {
                counts[index] += 1;
                let session = client.Session().map_err(|e| e.to_string())?;
                if session.SessionStatus().map_err(|e| e.to_string())? != GattSessionStatus::Active
                {
                    inactive_sessions += 1;
                    continue;
                }
                let id = session
                    .DeviceId()
                    .and_then(|d| d.Id())
                    .map_err(|e| e.to_string())?
                    .to_string();
                clients.insert(id, ());
            }
        }
        let values = self.values.lock().map_err(|_| "蓝牙报告状态不可用")?;
        let diagnostic = SubscriptionDiagnostic {
            counts,
            inactive_sessions,
            active_clients: clients.len(),
            read_requests: values.read_requests,
            write_requests: values.write_requests,
            subscription_changes: values.subscription_changes,
        };
        drop(values);
        if let Ok(mut previous) = self.last_subscription_diagnostic.lock()
            && previous.as_ref() != Some(&diagnostic)
        {
            tracing::info!(
                mouse = counts[0],
                keyboard = counts[1],
                consumer = counts[2],
                navigation = counts[3],
                boot_mouse = counts[4],
                boot_keyboard = counts[5],
                inactive_sessions,
                active_clients = diagnostic.active_clients,
                reads = diagnostic.read_requests,
                writes = diagnostic.write_requests,
                changes = diagnostic.subscription_changes,
                "BLE HID subscription snapshot"
            );
            *previous = Some(diagnostic);
        }
        Ok(clients.into_keys().collect())
    }

    fn last_advertisement_error(&self, status: GattServiceProviderAdvertisementStatus) -> i32 {
        self.advertisement_events
            .lock()
            .ok()
            .and_then(|events| {
                events
                    .iter()
                    .rev()
                    .find(|event| event.status == status.0)
                    .map(|event| event.error)
            })
            .unwrap_or(-1)
    }

    fn advertisement_trace(&self) -> String {
        self.advertisement_events
            .lock()
            .map(|events| {
                events
                    .iter()
                    .map(|event| format!("{}/{}", event.status, event.error))
                    .collect::<Vec<_>>()
                    .join(" → ")
            })
            .unwrap_or_else(|_| "不可用".into())
    }

    pub fn clients(&self) -> Result<Vec<ClientInfo>> {
        // Advertising and an established GATT connection are independent.
        // Keep a genuinely subscribed phone usable across Aborted/Success.
        let ids = self.active_client_ids()?;
        let status = self
            .provider
            .AdvertisementStatus()
            .map_err(|e| e.to_string())?;
        let mut unavailable_since = self
            .unavailable_since
            .lock()
            .map_err(|_| "蓝牙状态不可用")?;
        if is_advertising(status) || !ids.is_empty() {
            *unavailable_since = None;
        } else if unavailable_since.get_or_insert_with(Instant::now).elapsed()
            >= Duration::from_secs(5)
        {
            return Err(format!(
                "蓝牙广播与 HID 连接均已停止（状态 {}，{}；事件 {}）",
                status.0,
                bluetooth_error(self.last_advertisement_error(status)),
                self.advertisement_trace()
            ));
        }
        drop(unavailable_since);
        let mut clients = Vec::new();
        for id in ids {
            let mut names = self
                .names
                .lock()
                .map_err(|_| "蓝牙名称缓存不可用".to_owned())?;
            let name = names
                .entry(id.clone())
                .or_insert_with(|| {
                    device_name(&id).unwrap_or_else(|_| {
                        format!(
                            "蓝牙设备 · {}",
                            id.chars()
                                .rev()
                                .take(8)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect::<String>()
                        )
                    })
                })
                .clone();
            clients.push(ClientInfo { id, name });
        }
        Ok(clients)
    }

    pub fn send(
        &self,
        client_id: &str,
        report_id: u8,
        report: &[u8],
        timeout: Duration,
    ) -> Result<()> {
        self.send_if(client_id, report_id, report, timeout, &|| true)
    }

    pub fn send_if(
        &self,
        client_id: &str,
        report_id: u8,
        report: &[u8],
        timeout: Duration,
        admitted: &dyn Fn() -> bool,
    ) -> Result<()> {
        let normal = self.reports.get(&report_id).ok_or("未知 HID 报告")?;
        let mode = self
            .values
            .lock()
            .map_err(|_| "蓝牙报告状态不可用")?
            .clients
            .get(client_id)
            .map_or(1, |state| state.protocol_mode);
        let boot = match report_id {
            1 => Some(&self.boot_keyboard),
            2 => Some(&self.boot_mouse),
            _ => None,
        };
        let mut selected = None;
        let candidates = if mode == 0 {
            [boot, Some(normal)]
        } else {
            [Some(normal), boot]
        };
        for characteristic in candidates.into_iter().flatten() {
            let clients = characteristic
                .SubscribedClients()
                .map_err(|e| e.to_string())?;
            for client in clients {
                let session = client.Session().map_err(|e| e.to_string())?;
                if session.SessionStatus().map_err(|e| e.to_string())? != GattSessionStatus::Active
                {
                    continue;
                }
                let id = session
                    .DeviceId()
                    .and_then(|d| d.Id())
                    .map_err(|e| e.to_string())?;
                if id == client_id {
                    selected = Some((characteristic, client));
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        let Some((characteristic, client)) = selected else {
            if report_id == 4 || report_id == 5 {
                return Ok(());
            } // Optional consumer reports.
            return Err("已确认的手机没有订阅此键鼠报告".into());
        };
        let mut payload = if report_id == 2 && characteristic == &self.boot_mouse {
            boot_mouse_report(report)
        } else {
            report.to_vec()
        };
        if report_id == 2 && payload.len() == 6 {
            let multiplier = self
                .values
                .lock()
                .map_err(|_| "蓝牙报告状态不可用")?
                .clients
                .get(client_id)
                .map_or(1, |state| state.wheel_multiplier);
            payload[5] =
                (payload[5] as i8 as i16 * i16::from(multiplier)).clamp(-127, 127) as i8 as u8;
        }
        // Subscriber/session queries can take time. Recheck the generation
        // and foreground immediately before the native notification call.
        if !admitted() {
            return Ok(());
        }
        self.values
            .lock()
            .map_err(|_| "蓝牙报告状态不可用")?
            .reports
            .insert(report_id, report.to_vec());
        let result = wait!(
            characteristic.NotifyValueForSubscribedClientAsync(&buffer(&payload)?, &client),
            timeout
        )?;
        let status = result.Status().map_err(|e| e.to_string())?;
        if status != GattCommunicationStatus::Success {
            return Err(format!("通知手机失败（{}）", status.0));
        }
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let _ = self.provider.StopAdvertising();
        let _ = self
            .provider
            .RemoveAdvertisementStatusChanged(self.advertisement_token);
        // Closing each actual GATT session is essential after a transport
        // timeout: stopping advertising alone leaves existing links alive.
        for characteristic in self
            .reports
            .values()
            .chain([&self.boot_keyboard, &self.boot_mouse])
        {
            if let Ok(clients) = characteristic.SubscribedClients() {
                for client in clients {
                    if let Ok(session) = client.Session() {
                        let _ = session.Close();
                    }
                }
            }
        }
        self.characteristics.clear();
    }
}

fn bluetooth_error(error: i32) -> String {
    let name = match error {
        0 => "Success",
        1 => "RadioNotAvailable",
        2 => "ResourceInUse",
        3 => "DeviceNotConnected",
        4 => "OtherError",
        5 => "DisabledByPolicy",
        6 => "NotSupported",
        7 => "DisabledByUser",
        8 => "ConsentRequired",
        9 => "TransportNotSupported",
        _ => "Unknown",
    };
    format!("{name}: {error}")
}

fn device_name(id: &str) -> Result<String> {
    if let Ok(name) = bluetooth_device_name(id) {
        return Ok(name);
    }
    // Upstream falls back to the Windows device catalogue when the remote
    // Bluetooth object cannot be opened while iOS completes its connection.
    let information = wait!(
        DeviceInformation::CreateFromIdAsync(&HSTRING::from(id)),
        Duration::from_millis(500)
    )?;
    let name = information.Name().map_err(|e| e.to_string())?.to_string();
    if name.trim().is_empty() {
        Err("设备未提供名称".into())
    } else {
        Ok(name)
    }
}

fn bluetooth_device_name(id: &str) -> Result<String> {
    let device = wait!(
        BluetoothLEDevice::FromIdAsync(&HSTRING::from(id)),
        Duration::from_millis(500)
    )?;
    let name = device.Name().map_err(|e| e.to_string())?.to_string();
    let _ = device.Close();
    if name.trim().is_empty() {
        Err("设备未提供名称".into())
    } else {
        Ok(name)
    }
}

fn create(
    service: &GattLocalService,
    short_uuid: u16,
    properties: GattCharacteristicProperties,
    initial: &[u8],
    kind: ValueKind,
    values: &Arc<Mutex<Values>>,
    reference: Option<(u8, u8)>,
) -> Result<Characteristic> {
    let parameters = GattLocalCharacteristicParameters::new().map_err(|e| e.to_string())?;
    parameters
        .SetCharacteristicProperties(properties)
        .map_err(|e| e.to_string())?;
    // Windows serves StaticValue directly without raising ReadRequested.
    // Protocol mode, wheel multiplier and input reports must reflect current
    // per-client / report state instead of their initialization bytes.
    if matches!(kind, ValueKind::Static(_)) {
        parameters
            .SetStaticValue(&buffer(initial)?)
            .map_err(|e| e.to_string())?;
    }
    parameters
        .SetReadProtectionLevel(if properties.contains(GattCharacteristicProperties::Read) {
            GattProtectionLevel::EncryptionRequired
        } else {
            GattProtectionLevel::Plain
        })
        .map_err(|e| e.to_string())?;
    let writable = properties.contains(GattCharacteristicProperties::Write)
        || properties.contains(GattCharacteristicProperties::WriteWithoutResponse);
    parameters
        .SetWriteProtectionLevel(if writable {
            GattProtectionLevel::EncryptionRequired
        } else {
            GattProtectionLevel::Plain
        })
        .map_err(|e| e.to_string())?;
    let result = wait!(
        service.CreateCharacteristicAsync(uuid(short_uuid), &parameters),
        Duration::from_secs(3)
    )?;
    if result.Error().map_err(|e| e.to_string())? != BluetoothError::Success {
        return Err(format!("无法创建 HID 特征 {short_uuid:04X}"));
    }
    let value = result.Characteristic().map_err(|e| e.to_string())?;
    if let Some((report, report_type)) = reference {
        let descriptor = GattLocalDescriptorParameters::new().map_err(|e| e.to_string())?;
        descriptor
            .SetStaticValue(&buffer(&[report, report_type])?)
            .map_err(|e| e.to_string())?;
        descriptor
            .SetReadProtectionLevel(GattProtectionLevel::EncryptionRequired)
            .map_err(|e| e.to_string())?;
        descriptor
            .SetWriteProtectionLevel(GattProtectionLevel::EncryptionRequired)
            .map_err(|e| e.to_string())?;
        let result = wait!(
            value.CreateDescriptorAsync(uuid(0x2908), &descriptor),
            Duration::from_secs(3)
        )?;
        if result.Error().map_err(|e| e.to_string())? != BluetoothError::Success {
            return Err("无法创建 HID 报告描述符".into());
        }
    }
    let read_token = if properties.contains(GattCharacteristicProperties::Read) {
        let values = Arc::clone(values);
        let kind = kind.clone();
        Some(
            value
                .ReadRequested(&TypedEventHandler::<
                    GattLocalCharacteristic,
                    GattReadRequestedEventArgs,
                >::new(move |_, args| {
                    let args = args.ok()?.clone();
                    let deferral = args.GetDeferral()?;
                    let values = Arc::clone(&values);
                    let kind = kind.clone();
                    thread::spawn(move || {
                        let _ = initialize();
                        let result = (|| -> Result<()> {
                            let request = wait!(args.GetRequestAsync(), Duration::from_secs(2))?;
                            let client = args
                                .Session()
                                .and_then(|s| s.DeviceId())
                                .and_then(|d| d.Id())
                                .map(|id| id.to_string())
                                .unwrap_or_default();
                            let mut state = values.lock().map_err(|_| "蓝牙读取状态不可用")?;
                            state.read_requests += 1;
                            tracing::debug!(characteristic = short_uuid, "BLE HID read request");
                            let data = kind.read_value(&state, &client);
                            drop(state);
                            // Windows applies ATT offset and MTU to RespondWithValue.
                            request
                                .RespondWithValue(&buffer(&data)?)
                                .map_err(|e| e.to_string())
                        })();
                        if let Err(error) = result {
                            tracing::warn!("BLE HID read: {error}");
                        }
                        let _ = deferral.Complete();
                        uninitialize();
                    });
                    Ok(())
                }))
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let write_token = if writable {
        let values = Arc::clone(values);
        Some(
            value
                .WriteRequested(&TypedEventHandler::<
                    GattLocalCharacteristic,
                    GattWriteRequestedEventArgs,
                >::new(move |_, args| {
                    let args = args.ok()?.clone();
                    let deferral = args.GetDeferral()?;
                    let values = Arc::clone(&values);
                    let kind = kind.clone();
                    thread::spawn(move || {
                        let _ = initialize();
                        let result = (|| -> Result<()> {
                            let request = wait!(args.GetRequestAsync(), Duration::from_secs(2))?;
                            let client = args
                                .Session()
                                .and_then(|s| s.DeviceId())
                                .and_then(|d| d.Id())
                                .map(|id| id.to_string())
                                .unwrap_or_default();
                            let mut bytes = windows::core::Array::<u8>::new();
                            CryptographicBuffer::CopyToByteArray(
                                &request.Value().map_err(|e| e.to_string())?,
                                &mut bytes,
                            )
                            .map_err(|e| e.to_string())?;
                            {
                                let mut values = values.lock().map_err(|_| "蓝牙写入状态不可用")?;
                                values.write_requests += 1;
                            }
                            tracing::debug!(
                                characteristic = short_uuid,
                                bytes = bytes.len(),
                                "BLE HID write request"
                            );
                            if let Some(byte) = bytes.first() {
                                let mut values = values.lock().map_err(|_| "蓝牙写入状态不可用")?;
                                let state = values.clients.entry(client).or_insert(ClientState {
                                    protocol_mode: 1,
                                    wheel_multiplier: 1,
                                });
                                match kind {
                                    ValueKind::ProtocolMode if *byte <= 1 => {
                                        state.protocol_mode = *byte
                                    }
                                    ValueKind::WheelMultiplier => {
                                        state.wheel_multiplier = (*byte).clamp(1, 10)
                                    }
                                    _ => {}
                                }
                            }
                            if request.Option().map_err(|e| e.to_string())?
                                == GattWriteOption::WriteWithResponse
                            {
                                request.Respond().map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        })();
                        if let Err(error) = result {
                            tracing::warn!("BLE HID write: {error}");
                        }
                        let _ = deferral.Complete();
                        uninitialize();
                    });
                    Ok(())
                }))
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let subscribed_token = if properties.contains(GattCharacteristicProperties::Notify) {
        let values = Arc::clone(values);
        Some(
            value
                .SubscribedClientsChanged(&TypedEventHandler::<
                    GattLocalCharacteristic,
                    windows::core::IInspectable,
                >::new(move |_, _| {
                    // Never reenter the Bluetooth stack from its callback thread.
                    // The worker will enumerate subscriptions on its next poll.
                    if let Ok(mut values) = values.lock() {
                        values.subscription_changes += 1;
                    }
                    tracing::info!(
                        characteristic = short_uuid,
                        report = reference.map(|(id, _)| id),
                        "BLE HID subscription changed"
                    );
                    Ok(())
                }))
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    Ok(Characteristic {
        value,
        read_token,
        write_token,
        subscribed_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_uuid_expansion_matches_bluetooth_base() {
        assert_eq!(
            uuid(0x1812),
            GUID::from_u128(0x00001812_0000_1000_8000_00805f9b34fb)
        );
    }

    #[test]
    fn dynamic_reads_use_the_requesting_clients_negotiated_state() {
        let mut values = Values::default();
        values.clients.insert(
            "phone-a".into(),
            ClientState {
                protocol_mode: 0,
                wheel_multiplier: 8,
            },
        );
        assert_eq!(ValueKind::ProtocolMode.read_value(&values, "phone-a"), [0]);
        assert_eq!(
            ValueKind::WheelMultiplier.read_value(&values, "phone-a"),
            [8]
        );
        assert_eq!(ValueKind::ProtocolMode.read_value(&values, "phone-b"), [1]);
        assert_eq!(
            ValueKind::WheelMultiplier.read_value(&values, "phone-b"),
            [1]
        );
        values.reports.insert(1, vec![2, 0, 4, 0, 0, 0, 0, 0]);
        assert_eq!(
            ValueKind::Report(1).read_value(&values, "phone-a"),
            [2, 0, 4, 0, 0, 0, 0, 0]
        );
        values.reports.insert(1, vec![0; 8]);
        assert_eq!(ValueKind::Report(1).read_value(&values, "phone-a"), [0; 8]);
    }

    #[test]
    fn aborted_success_during_startup_waits_for_the_following_started_event() {
        assert_eq!(
            startup_decision(GattServiceProviderAdvertisementStatus::Aborted, 0, 0, false),
            StartupDecision::Waiting
        );
        assert_eq!(
            startup_decision(GattServiceProviderAdvertisementStatus::Started, 0, 0, false),
            StartupDecision::Advertising
        );
    }

    #[test]
    fn aborted_success_with_a_real_active_hid_subscription_is_usable() {
        assert_eq!(
            startup_decision(GattServiceProviderAdvertisementStatus::Aborted, 0, 1, false),
            StartupDecision::Connected
        );
        assert_eq!(
            startup_decision(GattServiceProviderAdvertisementStatus::Stopped, 0, 1, true),
            StartupDecision::Connected
        );
    }

    #[test]
    fn pairing_alone_cannot_turn_aborted_success_into_a_successful_start() {
        // A pairing record contributes no active subscription to this input.
        assert_eq!(
            startup_decision(GattServiceProviderAdvertisementStatus::Aborted, 0, 0, true),
            StartupDecision::Failed {
                status: 3,
                error: 0
            }
        );
    }

    #[test]
    fn advertising_without_all_ad_data_is_supported_like_upstream() {
        assert_eq!(
            startup_decision(
                GattServiceProviderAdvertisementStatus::StartedWithoutAllAdvertisementData,
                0,
                0,
                false
            ),
            StartupDecision::Advertising
        );
    }

    #[test]
    #[ignore = "Read-only Windows classic/BLE paired-device diagnostic; does not advertise, connect, pair or bind"]
    fn windows_paired_devices_readonly_diagnostic() {
        initialize().expect("WinRT initialization");
        let result = (|| -> Result<()> {
            for (kind, selector) in [
                (
                    "Classic",
                    windows::Devices::Bluetooth::BluetoothDevice::GetDeviceSelectorFromPairingState(
                        true,
                    ),
                ),
                (
                    "BLE",
                    BluetoothLEDevice::GetDeviceSelectorFromPairingState(true),
                ),
            ] {
                let selector = selector.map_err(|e| e.to_string())?;
                let devices = wait!(
                    DeviceInformation::FindAllAsyncAqsFilter(&selector),
                    Duration::from_secs(5)
                )?;
                println!(
                    "{kind} paired records: {}",
                    devices.Size().map_err(|e| e.to_string())?
                );
                for device in devices.into_iter().take(12) {
                    println!("  name: {}", device.Name().map_err(|e| e.to_string())?);
                }
            }
            Ok(())
        })();
        uninitialize();
        result.expect("Read-only device enumeration");
    }

    #[test]
    #[ignore = "Requires a Windows Bluetooth adapter; advertises HID briefly but does not pair or bind devices"]
    fn windows_service_start_stop_smoke() {
        initialize().expect("WinRT initialization");
        let result = Transport::start(|| false);
        match &result {
            Ok(transport) => println!(
                "Actual BLE HID service ready; advertisement events: {}; status: {:?}; subscribed clients: {:?}",
                transport.advertisement_trace(),
                transport.provider.AdvertisementStatus(),
                transport.clients()
            ),
            Err(error) => println!("Actual BLE capability result: {error}"),
        }
        let succeeded = result.is_ok();
        drop(result);
        uninitialize();
        assert!(
            succeeded,
            "The machine could not start a BLE HID peripheral service"
        );
    }
}
