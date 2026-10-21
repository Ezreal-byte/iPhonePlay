//! Exact Windows x64 C ABI 18 declarations. Do not replace UTF-16 with Rust chars.
use std::{ffi::c_void, mem::size_of, path::Path};

pub const API_VERSION: u32 = 18;
pub const BUFFER_TOO_SMALL: i32 = -3;
pub const NO_FRAME: i32 = -7;
pub type Handle = u64;

macro_rules! versioned {
    ($name:ident { $($field:ident : $kind:ty),* $(,)? }) => {
        #[repr(C)]
        #[derive(Clone, Debug)]
        pub struct $name {
            pub struct_size: u32,
            pub api_version: u32,
            $(pub $field: $kind),*
        }
        impl Default for $name {
            fn default() -> Self {
                // These FFI records contain only integer/float/array fields.
                let mut value: Self = unsafe { std::mem::zeroed() };
                value.struct_size = size_of::<Self>() as u32;
                value.api_version = API_VERSION;
                value
            }
        }
    }
}

versioned!(DeviceInfo {
    device_id: u32,
    mux_port: u32,
    state: i32,
    usb_connected: i32,
    pair_record_present: i32,
    lockdown_accessible: i32,
    udid: [u16; 128],
    name: [u16; 128],
    product_type: [u16; 64],
    os_version: [u16; 32],
    connection_type: [u16; 32],
    status: [u16; 192],
});
versioned!(EnvironmentInfo {
    apple_mobile_device_service_installed: i32,
    apple_mobile_device_service_running: i32,
    standard_usbmux_available: i32,
    capture_usbmux_available: i32,
    physical_apple_usb_devices: u32,
    diagnostic: [u16; 512],
    libusb_runtime_available: i32,
    usbdk_backend_available: i32,
    libusb_apple_devices: u32,
    libusb_version: [u16; 32],
    usbdk_backend_known: i32,
    libusb_apple_devices_known: i32,
});
versioned!(CaptureStatus {
    state: i32,
    width: u32,
    height: u32,
    fps: f64,
    latency_ms: f64,
    video_frames: u64,
    audio_packets: u64,
    audio_sample_rate: u32,
    audio_channels: u32,
    failure_kind: i32,
    failure_stage: i32,
    error_code: i32,
    message: [u16; 192],
});
versioned!(VideoOutputStatus {
    monitor_hdr_capability: u32,
    source_hdr_known: i32,
    source_hdr: i32,
    actual_hdr_surface: i32,
    hdr_effective: i32,
    requested_color_output_preference: u32,
    requested_decoder_preference: u32,
    applied_decoder_preference: u32,
    decoder_switch_state: u32,
    decoder_runtime_mode: u32,
    requested_decoder_generation: u64,
    applied_decoder_generation: u64,
});
versioned!(VideoFrameInfo {
    width: u32,
    height: u32,
    stride: u32,
    pixel_format: u32,
    timestamp_100ns: i64,
});
versioned!(AudioPacketInfo {
    sequence: u64,
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
});
versioned!(CaptureOptions {
    requested_width: u32,
    requested_height: u32,
    target_fps: u32,
    play_audio: i32,
    audio_volume: f32,
    reserved: [u32; 5],
});

pub type Refresh = unsafe extern "C" fn(*mut DeviceInfo, *mut u32) -> i32;
pub type CopyVideo =
    unsafe extern "C" fn(Handle, *mut VideoFrameInfo, *mut u8, *mut u32, u32, u32) -> i32;

macro_rules! api {
    ($($field:ident : $kind:ty => $symbol:literal),* $(,)?) => {
        pub struct Api {
            // The library must outlive every copied function pointer.
            _library: libloading::Library,
            $(pub $field: $kind),*
        }
        impl Api {
            pub fn load(path: &Path) -> Result<Self, String> {
                // Resolve only in the DLL's directory and safe default system dirs.
                #[cfg(windows)]
                let library: libloading::Library = unsafe {
                    libloading::os::windows::Library::load_with_flags(path, 0x100 | 0x1000)
                }.map_err(|e| format!("无法加载投屏核心 {}：{e}", path.display()))?.into();
                #[cfg(not(windows))]
                let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
                unsafe {
                    $(let $field: $kind = *library.get::<$kind>(concat!($symbol, "\0").as_bytes())
                        .map_err(|e| format!("ABI 18 缺少导出 {}：{e}", $symbol))?;)*
                    Ok(Self { _library: library, $($field),* })
                }
            }
        }
    }
}

api! {
    initialize: unsafe extern "C" fn() -> i32 => "im_initialize",
    shutdown: unsafe extern "C" fn() -> () => "im_shutdown",
    version: unsafe extern "C" fn() -> u32 => "im_api_version",
    last_error: unsafe extern "C" fn() -> *const u16 => "im_last_error",
    receiver_start: unsafe extern "C" fn(*const u16, *const u16, u32, u32, u32) -> i32 => "im_wireless_receiver_start_ex",
    receiver_stop: unsafe extern "C" fn() -> () => "im_wireless_receiver_stop",
    receiver_status: unsafe extern "C" fn(*mut i32, *mut i32) -> i32 => "im_wireless_receiver_get_status",
    refresh_usb: Refresh => "im_refresh_devices",
    refresh_usb_metadata: unsafe extern "C" fn(*mut DeviceInfo, *mut u32, i32) -> i32 => "im_refresh_devices_ex",
    refresh_wireless: Refresh => "im_refresh_wireless_devices",
    environment: unsafe extern "C" fn(*mut EnvironmentInfo) -> i32 => "im_get_environment",
    usb_driver_available: unsafe extern "C" fn(*const u16, *mut i32) -> i32 => "im_is_libusb0_device_available",
    usb_create: unsafe extern "C" fn(*const u16, *const CaptureOptions, *mut Handle) -> i32 => "im_session_create",
    wireless_create: unsafe extern "C" fn(*const u16, *const CaptureOptions, *mut Handle) -> i32 => "im_wireless_session_create",
    stop: unsafe extern "C" fn(Handle) -> i32 => "im_session_stop",
    destroy: unsafe extern "C" fn(Handle) -> () => "im_session_destroy",
    status: unsafe extern "C" fn(Handle, *mut CaptureStatus) -> i32 => "im_session_get_status",
    video_status: unsafe extern "C" fn(Handle, *mut c_void, *mut VideoOutputStatus) -> i32 => "im_session_get_video_output_status",
    attach: unsafe extern "C" fn(Handle, *mut c_void) -> i32 => "im_session_attach_preview",
    detach: unsafe extern "C" fn(Handle, *mut c_void) -> () => "im_session_detach_preview",
    video_preferences: unsafe extern "C" fn(Handle, u32, u32, u32) -> i32 => "im_session_set_video_preferences",
    audio_enabled: unsafe extern "C" fn(Handle, i32) -> i32 => "im_session_set_audio_enabled",
    audio_volume: unsafe extern "C" fn(Handle, f32) -> i32 => "im_session_set_audio_volume",
    corner_profile: unsafe extern "C" fn(Handle, f32, f32) -> i32 => "im_session_set_corner_profile",
    timestamp: unsafe extern "C" fn(Handle, *mut i64) -> i32 => "im_session_get_latest_video_timestamp",
    copy_bgra: CopyVideo => "im_session_copy_latest_video_frame",
    copy_nv12: CopyVideo => "im_session_copy_latest_video_frame_nv12",
    copy_audio: unsafe extern "C" fn(Handle, u64, *mut AudioPacketInfo, *mut u8, *mut u32) -> i32 => "im_session_copy_next_audio_packet",
    refresh_preview: unsafe extern "C" fn(Handle) -> i32 => "im_session_force_preview_refresh",
}

pub fn wide(value: &str) -> Result<Vec<u16>, String> {
    if value.contains('\0') {
        return Err("文字中不能包含空字符".into());
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}
pub fn wide_path(path: &Path) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;
    let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err("路径中不能包含空字符".into());
    }
    units.push(0);
    Ok(units)
}
pub fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|&v| v == 0).unwrap_or(value.len())])
}
pub fn validate_header(size: u32, version: u32, expected: usize) -> Result<(), String> {
    if size as usize != expected || version != API_VERSION {
        Err(format!(
            "投屏核心结构不兼容：ABI {version}，结构 {size} 字节，预期 ABI {API_VERSION} / {expected} 字节"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of};
    #[test]
    fn windows_x64_abi18_layout() {
        assert_eq!(size_of::<DeviceInfo>(), 1184);
        assert_eq!(size_of::<EnvironmentInfo>(), 1136);
        assert_eq!(size_of::<CaptureStatus>(), 464);
        assert_eq!(size_of::<VideoOutputStatus>(), 64);
        assert_eq!(size_of::<CaptureOptions>(), 48);
        assert_eq!(size_of::<VideoFrameInfo>(), 32);
        assert_eq!(size_of::<AudioPacketInfo>(), 24);
        assert_eq!(offset_of!(CaptureStatus, fps), 24);
        assert_eq!(offset_of!(CaptureStatus, message), 76);
        assert_eq!(offset_of!(DeviceInfo, udid), 32);
        assert_eq!(offset_of!(EnvironmentInfo, libusb_runtime_available), 1052);
        assert_eq!(align_of::<CaptureStatus>(), 8);
    }
    #[test]
    fn rejects_header_mismatch_and_interior_nul() {
        assert!(validate_header(48, 17, 48).is_err());
        assert!(validate_header(44, 18, 48).is_err());
        assert!(wide("abc\0def").is_err());
        assert_eq!(
            text(&wide("iPhonePlay · 中文").unwrap()),
            "iPhonePlay · 中文"
        );
    }
}
