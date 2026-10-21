use super::ffi;
use std::{
    fmt,
    mem::size_of,
    path::Path,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

static CORE_IN_USE: AtomicBool = AtomicBool::new(false);
const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;
const MAX_AUDIO_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct Core {
    pub api: ffi::Api,
}
impl Core {
    pub fn load(path: &Path) -> Result<Arc<Self>, String> {
        if CORE_IN_USE.swap(true, Ordering::AcqRel) {
            return Err("投屏核心已经由另一个后端实例使用".into());
        }
        let result = (|| {
            let api = ffi::Api::load(path)?;
            let version = unsafe { (api.version)() };
            if version != ffi::API_VERSION {
                return Err(format!(
                    "投屏核心 ABI {version} 不兼容，需要 ABI {}",
                    ffi::API_VERSION
                ));
            }
            let rc = unsafe { (api.initialize)() };
            if rc != 0 {
                return Err(format!("投屏核心初始化失败（{rc}）"));
            }
            Ok(Arc::new(Self { api }))
        })();
        if result.is_err() {
            CORE_IN_USE.store(false, Ordering::Release);
        }
        result
    }
    pub fn check(&self, rc: i32, action: &str) -> Result<(), String> {
        if rc == 0 {
            return Ok(());
        }
        let message = unsafe {
            let ptr = (self.api.last_error)();
            if ptr.is_null() {
                String::new()
            } else {
                // im_last_error returns a thread-owned UTF-16 NUL-terminated string.
                let mut len = 0;
                while len < 4096 && *ptr.add(len) != 0 {
                    len += 1;
                }
                ffi::text(std::slice::from_raw_parts(ptr, len))
            }
        };
        Err(format!("{action}失败（{rc}）：{message}"))
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            (self.api.receiver_stop)();
            (self.api.shutdown)();
        }
        CORE_IN_USE.store(false, Ordering::Release);
    }
}

/// A shared, lifetime-safe handle. The native core internally serializes session
/// lifecycle and retains frames while copying; no native memory is borrowed.
pub struct NativeSession {
    pub(crate) core: Arc<Core>,
    pub(crate) handle: ffi::Handle,
    stopped: AtomicBool,
}
impl fmt::Debug for NativeSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeSession")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionStatus {
    /// Core ABI state: 0 idle, 1 activating, 2 waiting, 3 handshake,
    /// 4 streaming, 5 stopping, 6 stopped, 7 error.
    pub state: i32,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub latency_ms: f64,
    pub video_frames: u64,
    pub audio_packets: u64,
    pub audio_sample_rate: u32,
    pub audio_channels: u32,
    pub failure_kind: i32,
    pub failure_stage: i32,
    pub error_code: i32,
    pub message: String,
}
impl SessionStatus {
    pub fn has_video(&self) -> bool {
        self.state == 4 && self.width > 0 && self.height > 0 && self.video_frames > 0
    }
    pub fn is_terminal(&self) -> bool {
        self.state == 6 || self.state == 7
    }
}

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: u32,
    pub timestamp_100ns: i64,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct AudioPacket {
    pub sequence: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub data: Vec<u8>,
}

impl NativeSession {
    pub(crate) fn new(core: Arc<Core>, handle: ffi::Handle) -> Self {
        Self {
            core,
            handle,
            stopped: AtomicBool::new(false),
        }
    }
    pub fn status(&self) -> Result<SessionStatus, String> {
        let mut raw = ffi::CaptureStatus::default();
        self.core.check(
            unsafe { (self.core.api.status)(self.handle, &mut raw) },
            "获取投屏状态",
        )?;
        ffi::validate_header(
            raw.struct_size,
            raw.api_version,
            size_of::<ffi::CaptureStatus>(),
        )?;
        Ok(SessionStatus {
            state: raw.state,
            width: raw.width,
            height: raw.height,
            fps: raw.fps,
            latency_ms: raw.latency_ms,
            video_frames: raw.video_frames,
            audio_packets: raw.audio_packets,
            audio_sample_rate: raw.audio_sample_rate,
            audio_channels: raw.audio_channels,
            failure_kind: raw.failure_kind,
            failure_stage: raw.failure_stage,
            error_code: raw.error_code,
            message: ffi::text(&raw.message),
        })
    }
    pub fn latest_timestamp(&self) -> Result<i64, String> {
        let mut timestamp = 0;
        self.core.check(
            unsafe { (self.core.api.timestamp)(self.handle, &mut timestamp) },
            "获取视频时间戳",
        )?;
        Ok(timestamp)
    }
    pub fn video_output_status(&self, hwnd: usize) -> Result<ffi::VideoOutputStatus, String> {
        let mut status = ffi::VideoOutputStatus::default();
        self.core.check(
            unsafe { (self.core.api.video_status)(self.handle, hwnd as *mut _, &mut status) },
            "获取视频输出诊断",
        )?;
        ffi::validate_header(
            status.struct_size,
            status.api_version,
            size_of::<ffi::VideoOutputStatus>(),
        )?;
        Ok(status)
    }
    pub fn bgra_frame(&self) -> Result<Option<VideoFrame>, String> {
        self.copy_video(self.core.api.copy_bgra, 0, 0, 1)
    }
    pub fn nv12_frame(&self, width: u32, height: u32) -> Result<Option<VideoFrame>, String> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err("NV12 画布尺寸必须为非零偶数".into());
        }
        self.copy_video(self.core.api.copy_nv12, width, height, 2)
    }
    fn copy_video(
        &self,
        copy: ffi::CopyVideo,
        width: u32,
        height: u32,
        expected_format: u32,
    ) -> Result<Option<VideoFrame>, String> {
        let mut info = ffi::VideoFrameInfo::default();
        let mut size = 0;
        let rc = unsafe {
            copy(
                self.handle,
                &mut info,
                ptr::null_mut(),
                &mut size,
                width,
                height,
            )
        };
        if rc == ffi::NO_FRAME {
            return Ok(None);
        }
        if rc != ffi::BUFFER_TOO_SMALL {
            self.core.check(rc, "查询视频帧")?;
        }
        let mut data = Vec::new();
        // A rotation may change BGRA dimensions between query and copy. Retry
        // with the reported capacity; never expose partially written data.
        for _ in 0..4 {
            if size == 0 || size as usize > MAX_FRAME_BYTES {
                return Err("视频帧缓冲区尺寸无效".into());
            }
            data.resize(size as usize, 0);
            let mut capacity = size;
            let rc = unsafe {
                copy(
                    self.handle,
                    &mut info,
                    data.as_mut_ptr(),
                    &mut capacity,
                    width,
                    height,
                )
            };
            if rc == ffi::NO_FRAME {
                return Ok(None);
            }
            if rc == ffi::BUFFER_TOO_SMALL {
                size = capacity;
                continue;
            }
            self.core.check(rc, "导出视频帧")?;
            ffi::validate_header(
                info.struct_size,
                info.api_version,
                size_of::<ffi::VideoFrameInfo>(),
            )?;
            if capacity as usize > data.len()
                || info.pixel_format != expected_format
                || info.width == 0
                || info.height == 0
            {
                return Err("核心返回了无效的视频帧描述".into());
            }
            let expected = if expected_format == 1 {
                u64::from(info.stride) * u64::from(info.height)
            } else {
                u64::from(info.width) * u64::from(info.height) * 3 / 2
            };
            if expected != u64::from(capacity) {
                return Err("视频帧长度与描述不一致".into());
            }
            data.truncate(capacity as usize);
            return Ok(Some(VideoFrame {
                width: info.width,
                height: info.height,
                stride: info.stride,
                pixel_format: info.pixel_format,
                timestamp_100ns: info.timestamp_100ns,
                data,
            }));
        }
        Err("视频尺寸正在变化，请重试".into())
    }
    pub fn audio_packet(&self, after_sequence: u64) -> Result<Option<AudioPacket>, String> {
        let mut info = ffi::AudioPacketInfo::default();
        let mut size = 0;
        let copy = self.core.api.copy_audio;
        let rc = unsafe {
            copy(
                self.handle,
                after_sequence,
                &mut info,
                ptr::null_mut(),
                &mut size,
            )
        };
        if rc == ffi::NO_FRAME {
            return Ok(None);
        }
        if rc != ffi::BUFFER_TOO_SMALL {
            self.core.check(rc, "查询手机声音")?;
        }
        let mut data = Vec::new();
        for _ in 0..4 {
            if size as usize > MAX_AUDIO_BYTES {
                return Err("手机音频数据包过大".into());
            }
            data.resize(size as usize, 0);
            let mut capacity = size;
            let rc = unsafe {
                copy(
                    self.handle,
                    after_sequence,
                    &mut info,
                    data.as_mut_ptr(),
                    &mut capacity,
                )
            };
            if rc == ffi::NO_FRAME {
                return Ok(None);
            }
            if rc == ffi::BUFFER_TOO_SMALL {
                size = capacity;
                continue;
            }
            self.core.check(rc, "导出手机声音")?;
            ffi::validate_header(
                info.struct_size,
                info.api_version,
                size_of::<ffi::AudioPacketInfo>(),
            )?;
            if capacity as usize > data.len()
                || info.sequence <= after_sequence
                || info.channels == 0
                || info.sample_rate == 0
            {
                return Err("核心返回了无效的音频数据包".into());
            }
            data.truncate(capacity as usize);
            return Ok(Some(AudioPacket {
                sequence: info.sequence,
                sample_rate: info.sample_rate,
                channels: info.channels,
                bits_per_sample: info.bits_per_sample,
                data,
            }));
        }
        Err("音频缓冲区正在变化，请重试".into())
    }
    pub(crate) fn stop(&self) -> Result<(), String> {
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        self.core
            .check(unsafe { (self.core.api.stop)(self.handle) }, "结束投屏会话")?;
        self.stopped.store(true, Ordering::Release);
        Ok(())
    }
    pub(crate) fn attach(&self, hwnd: usize) -> Result<(), String> {
        if hwnd == 0 {
            return Err("预览窗口尚未建立".into());
        }
        self.core.check(
            unsafe { (self.core.api.corner_profile)(self.handle, 0.0, 2.0) },
            "关闭额外画面裁切",
        )?;
        self.core.check(
            unsafe { (self.core.api.attach)(self.handle, hwnd as *mut _) },
            "建立 D3D11 预览",
        )
    }
    pub(crate) fn detach(&self, hwnd: usize) {
        if hwnd != 0 {
            unsafe { (self.core.api.detach)(self.handle, hwnd as *mut _) };
        }
    }
    pub(crate) fn audio(&self, enabled: bool, volume: f32) -> Result<(), String> {
        self.core.check(
            unsafe { (self.core.api.audio_enabled)(self.handle, i32::from(enabled)) },
            "设置手机声音播放",
        )?;
        self.core.check(
            unsafe { (self.core.api.audio_volume)(self.handle, volume.clamp(0.0, 1.0)) },
            "设置手机声音音量",
        )
    }
    pub(crate) fn preview(&self, width: u32, height: u32, fps: u32) -> Result<(), String> {
        self.core.check(
            unsafe { (self.core.api.video_preferences)(self.handle, width, height, fps) },
            "设置本地预览",
        )
    }
    pub(crate) fn refresh_preview(&self) -> Result<(), String> {
        self.core.check(
            unsafe { (self.core.api.refresh_preview)(self.handle) },
            "刷新原生预览",
        )
    }
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        // Native destroy releases its renderers and stops capture even if a
        // previous explicit stop reported a teardown warning.
        unsafe { (self.core.api.destroy)(self.handle) };
    }
}
