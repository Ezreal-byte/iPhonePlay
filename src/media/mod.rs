//! Original-frame screenshots and wall-clock scheduled MP4 recording.
//!
//! The UI never writes a pipe or waits for FFmpeg. A session retains its frame
//! source until finalization completes; callers must stop recording before
//! stopping the capture session. Preview mute/volume are deliberately absent.
#[cfg(test)]
mod integration;
mod pipe;
mod recording;
mod timing;

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub use timing::recording_dimensions;

#[derive(Clone, Debug)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub timestamp_100ns: i64,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct BgraFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
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

/// Implementations return owned copies and support concurrent video/audio reads.
pub trait FrameSource: Send + Sync + 'static {
    fn dimensions(&self) -> Result<(u32, u32), String>;
    /// Exact even-sized NV12 canvas, with source fitted/letterboxed by the core.
    fn nv12(&self, width: u32, height: u32) -> Result<Option<VideoFrame>, String>;
    fn bgra(&self) -> Result<Option<BgraFrame>, String>;
    fn audio(&self, after_sequence: u64) -> Result<Option<AudioPacket>, String>;
}

impl FrameSource for crate::native::NativeSession {
    fn dimensions(&self) -> Result<(u32, u32), String> {
        let status = self.status()?;
        Ok((status.width, status.height))
    }

    fn nv12(&self, width: u32, height: u32) -> Result<Option<VideoFrame>, String> {
        self.nv12_frame(width, height).map(|frame| {
            frame.map(|frame| VideoFrame {
                width: frame.width,
                height: frame.height,
                stride: frame.stride,
                timestamp_100ns: frame.timestamp_100ns,
                data: frame.data,
            })
        })
    }

    fn bgra(&self) -> Result<Option<BgraFrame>, String> {
        self.bgra_frame().map(|frame| {
            frame.map(|frame| BgraFrame {
                width: frame.width,
                height: frame.height,
                stride: frame.stride,
                data: frame.data,
            })
        })
    }

    fn audio(&self, after_sequence: u64) -> Result<Option<AudioPacket>, String> {
        self.audio_packet(after_sequence).map(|packet| {
            packet.map(|packet| AudioPacket {
                sequence: packet.sequence,
                sample_rate: packet.sample_rate,
                channels: packet.channels,
                bits_per_sample: packet.bits_per_sample,
                data: packet.data,
            })
        })
    }
}

#[derive(Clone, Debug)]
pub struct RecordingOptions {
    pub directory: PathBuf,
    pub max_long_edge: u32,
    pub fps: u32,
    pub record_audio: bool,
    pub bitrate_kbps: u32,
}

impl Default for RecordingOptions {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("."),
            max_long_edge: 1920,
            fps: 60,
            record_audio: true,
            bitrate_kbps: 12_000,
        }
    }
}

#[derive(Clone, Debug)]
pub enum MediaEvent {
    Starting {
        generation: u64,
    },
    Started {
        generation: u64,
        temporary_path: PathBuf,
        encoder: String,
        started_at: Instant,
    },
    Finalizing {
        generation: u64,
    },
    Saved {
        generation: u64,
        path: PathBuf,
    },
    Failed {
        generation: u64,
        message: String,
        partial_path: Option<PathBuf>,
    },
    Cancelled {
        generation: u64,
    },
    ScreenshotSaved {
        generation: u64,
        path: PathBuf,
    },
    ScreenshotFailed {
        generation: u64,
        message: String,
    },
}

impl MediaEvent {
    pub fn generation(&self) -> u64 {
        match self {
            Self::Starting { generation }
            | Self::Started { generation, .. }
            | Self::Finalizing { generation }
            | Self::Saved { generation, .. }
            | Self::Failed { generation, .. }
            | Self::Cancelled { generation }
            | Self::ScreenshotSaved { generation, .. }
            | Self::ScreenshotFailed { generation, .. } => *generation,
        }
    }
}

/// Worker control is independent from the UI's event receiver. Stop is latched
/// once at its exact monotonic time, so neither writer shortens elapsed time.
pub(super) struct SessionControl {
    stopped: AtomicBool,
    abort: AtomicBool,
    clock: Mutex<Option<(Instant, Option<Duration>)>>,
}

impl SessionControl {
    fn new() -> Self {
        Self {
            stopped: AtomicBool::new(false),
            abort: AtomicBool::new(false),
            clock: Mutex::new(None),
        }
    }

    fn stop(&self) {
        let mut clock = self.clock.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((start, stop)) = clock.as_mut() {
            stop.get_or_insert_with(|| start.elapsed());
        }
        self.stopped.store(true, Ordering::Release);
    }

    fn begin(&self) -> Instant {
        let start = Instant::now();
        let mut clock = self.clock.lock().unwrap_or_else(|p| p.into_inner());
        // A stop can arrive between the last startup check and process spawn.
        // Preserve it rather than accidentally starting a moving stop horizon.
        *clock = Some((start, self.stopping().then_some(Duration::ZERO)));
        start
    }

    fn elapsed(&self) -> Duration {
        self.clock
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map_or(Duration::ZERO, |(start, stop)| {
                stop.unwrap_or_else(|| start.elapsed())
            })
    }

    fn stopping(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn aborting(&self) -> bool {
        self.abort.load(Ordering::Acquire)
    }
}

pub struct Recorder {
    ffmpeg: PathBuf,
    events_tx: mpsc::Sender<MediaEvent>,
    events: mpsc::Receiver<MediaEvent>,
    current: Option<(Arc<SessionControl>, JoinHandle<()>)>,
    busy: Arc<AtomicBool>,
    screenshot_busy: Arc<AtomicBool>,
}

impl Recorder {
    pub fn new(ffmpeg: PathBuf) -> Self {
        let (events_tx, events) = mpsc::channel();
        Self {
            ffmpeg,
            events_tx,
            events,
            current: None,
            busy: Arc::new(AtomicBool::new(false)),
            screenshot_busy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    pub fn try_event(&self) -> Option<MediaEvent> {
        self.events.try_recv().ok()
    }

    pub fn start(
        &mut self,
        source: Arc<dyn FrameSource>,
        options: RecordingOptions,
        generation: u64,
    ) -> Result<(), String> {
        if !matches!(options.fps, 30 | 60) {
            return Err("录制帧率只能为 30 或 60 FPS".into());
        }
        if !(320..=3840).contains(&options.max_long_edge)
            || !(500..=100_000).contains(&options.bitrate_kbps)
        {
            return Err("录制分辨率或码率超出支持范围".into());
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err("正在录制或完成封装，请稍候".into());
        }
        if let Some((_, previous)) = self.current.take() {
            let _ = previous.join();
        }
        let control = Arc::new(SessionControl::new());
        let worker_control = Arc::clone(&control);
        let ffmpeg = self.ffmpeg.clone();
        let tx = self.events_tx.clone();
        let busy = Arc::clone(&self.busy);
        let worker = thread::Builder::new()
            .name("iphoneplay-recorder".into())
            .spawn(move || {
                let _ = tx.send(MediaEvent::Starting { generation });
                recording::run(ffmpeg, source, options, generation, worker_control, tx);
                busy.store(false, Ordering::Release);
            });
        match worker {
            Ok(worker) => {
                self.current = Some((control, worker));
                Ok(())
            }
            Err(error) => {
                self.busy.store(false, Ordering::Release);
                Err(format!("无法启动录制任务：{error}"))
            }
        }
    }

    /// Asynchronous; wait for Saved/Failed/Cancelled before releasing capture.
    pub fn stop(&self) {
        if let Some((control, _)) = &self.current {
            control.stop();
        }
    }

    pub fn screenshot(
        &self,
        source: Arc<dyn FrameSource>,
        directory: PathBuf,
        generation: u64,
    ) -> Result<(), String> {
        if self.screenshot_busy.swap(true, Ordering::AcqRel) {
            return Err("正在保存上一张截图".into());
        }
        let tx = self.events_tx.clone();
        let busy = Arc::clone(&self.screenshot_busy);
        match thread::Builder::new()
            .name("iphoneplay-screenshot".into())
            .spawn(move || {
                let result = source
                    .bgra()
                    .and_then(|frame| frame.ok_or_else(|| "尚未收到可用于截图的画面".to_string()))
                    .and_then(|frame| save_png(&frame, &directory));
                let event = match result {
                    Ok(path) => MediaEvent::ScreenshotSaved { generation, path },
                    Err(message) => MediaEvent::ScreenshotFailed {
                        generation,
                        message,
                    },
                };
                let _ = tx.send(event);
                busy.store(false, Ordering::Release);
            }) {
            Ok(_) => Ok(()),
            Err(error) => {
                self.screenshot_busy.store(false, Ordering::Release);
                Err(format!("无法启动截图任务：{error}"))
            }
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Normal exit first awaits the terminal event in the app state machine.
        // Also request safe finalization on abnormal owner teardown; never block
        // the window thread for the two-minute mux timeout.
        self.stop();
    }
}

static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn unique_stem(prefix: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S_%3f");
    format!(
        "{prefix}_{stamp}_{}",
        FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn reserve_output(
    directory: &Path,
    prefix: &str,
    extension: &str,
) -> Result<(PathBuf, File), String> {
    fs::create_dir_all(directory).map_err(|e| format!("无法创建保存目录：{e}"))?;
    for _ in 0..16 {
        let path = directory.join(format!("{}.{extension}", unique_stem(prefix)));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("保存目录不可写或磁盘空间不足：{e}")),
        }
    }
    Err("无法创建唯一的输出文件名".into())
}

fn bgra_to_rgba(frame: &BgraFrame) -> Result<Vec<u8>, String> {
    let row_bytes = (frame.width as usize)
        .checked_mul(4)
        .ok_or("截图尺寸溢出")?;
    let required = (frame.stride as usize)
        .checked_mul(frame.height as usize)
        .ok_or("截图尺寸溢出")?;
    if frame.width == 0
        || frame.height == 0
        || frame.width > 16384
        || frame.height > 16384
        || (frame.stride as usize) < row_bytes
        || frame.data.len() < required
    {
        return Err("原始截图帧的尺寸或缓冲区无效".into());
    }
    let mut rgba = Vec::with_capacity(row_bytes * frame.height as usize);
    for row in frame
        .data
        .chunks_exact(frame.stride as usize)
        .take(frame.height as usize)
    {
        for bgra in row[..row_bytes].chunks_exact(4) {
            rgba.extend_from_slice(&[bgra[2], bgra[1], bgra[0], 255]);
        }
    }
    Ok(rgba)
}

pub fn save_png(frame: &BgraFrame, directory: &Path) -> Result<PathBuf, String> {
    let rgba = bgra_to_rgba(frame)?;
    let (partial, file) = reserve_output(directory, "iPhonePlay", "partial.png")?;
    let final_path = partial.with_file_name(
        partial
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(".partial.png", ".png"),
    );
    let result = (|| {
        let mut output = BufWriter::new(file);
        {
            let mut encoder = png::Encoder::new(&mut output, frame.width, frame.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?;
        }
        output.flush().map_err(|e| e.to_string())?;
        output.get_ref().sync_all().map_err(|e| e.to_string())?;
        drop(output);
        publish_file(&partial, &final_path)
    })();
    result.map_err(|e| format!("截图保存失败：{e}；临时文件：{}", partial.display()))?;
    Ok(final_path)
}

/// Atomically create the final directory entry without overwriting an existing
/// user file. Temporary files share the destination directory/volume.
fn publish_file(temporary: &Path, final_path: &Path) -> Result<(), String> {
    if final_path.exists() {
        return Err("目标文件已存在，未覆盖已有文件".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::MoveFileW;
        use windows::core::PCWSTR;
        let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = final_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // MoveFileW fails instead of replacing an existing destination.
        unsafe { MoveFileW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr())) }
            .map_err(|e| format!("完成文件命名失败：{e}"))?;
    }
    #[cfg(not(windows))]
    {
        fs::hard_link(temporary, final_path).map_err(|e| e.to_string())?;
        fs::remove_file(temporary).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_ignores_stride_padding_and_forces_opaque_source() {
        let frame = BgraFrame {
            width: 1,
            height: 2,
            stride: 8,
            data: vec![3, 2, 1, 0, 9, 9, 9, 9, 6, 5, 4, 255, 9, 9, 9, 9],
        };
        assert_eq!(bgra_to_rgba(&frame).unwrap(), [1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn short_frame_is_rejected_before_writing() {
        let frame = BgraFrame {
            width: 10,
            height: 10,
            stride: 40,
            data: vec![0; 399],
        };
        assert!(bgra_to_rgba(&frame).is_err());
    }

    #[test]
    fn finalize_never_overwrites_user_file() {
        let directory = std::env::temp_dir().join(unique_stem("iphoneplay-publish-test"));
        fs::create_dir_all(&directory).unwrap();
        let temporary = directory.join("temporary");
        let final_path = directory.join("final");
        fs::write(&temporary, b"new").unwrap();
        fs::write(&final_path, b"existing").unwrap();
        assert!(publish_file(&temporary, &final_path).is_err());
        assert_eq!(fs::read(&temporary).unwrap(), b"new");
        assert_eq!(fs::read(&final_path).unwrap(), b"existing");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn stop_time_does_not_advance_during_finalization() {
        let control = SessionControl::new();
        control.begin();
        control.stop();
        let stopped = control.elapsed();
        thread::sleep(Duration::from_millis(5));
        control.stop();
        assert_eq!(control.elapsed(), stopped);
    }

    #[test]
    fn stop_during_startup_does_not_restart_the_clock() {
        let control = SessionControl::new();
        control.stop();
        control.begin();
        assert!(control.stopping());
        assert_eq!(control.elapsed(), Duration::ZERO);
    }
}
