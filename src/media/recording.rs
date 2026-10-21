use super::pipe::OutputPipe;
use super::timing::{PcmNormalizer, recording_dimensions, units_due, video_frames_due};
use super::{
    FrameSource, MediaEvent, RecordingOptions, SessionControl, VideoFrame, publish_file,
    reserve_output,
};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const FINALIZE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
struct Failure {
    message: String,
    partial: Option<PathBuf>,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            partial: None,
        }
    }
}

pub(super) fn run(
    ffmpeg: PathBuf,
    source: Arc<dyn FrameSource>,
    options: RecordingOptions,
    generation: u64,
    control: Arc<SessionControl>,
    events: mpsc::Sender<MediaEvent>,
) {
    let result = run_inner(&ffmpeg, source, options, generation, &control, &events);
    let event = match result {
        Ok(Some(path)) => MediaEvent::Saved { generation, path },
        Ok(None) => MediaEvent::Cancelled { generation },
        Err(failure) => MediaEvent::Failed {
            generation,
            message: failure.message,
            partial_path: failure.partial,
        },
    };
    let _ = events.send(event);
}

fn run_inner(
    ffmpeg: &Path,
    source: Arc<dyn FrameSource>,
    options: RecordingOptions,
    generation: u64,
    control: &Arc<SessionControl>,
    events: &mpsc::Sender<MediaEvent>,
) -> Result<Option<PathBuf>, Failure> {
    if !ffmpeg.is_file() {
        return Err(format!("缺少 FFmpeg，请在依赖检查中修复：{}", ffmpeg.display()).into());
    }
    let (source_width, source_height) = source.dimensions()?;
    let (width, height) = recording_dimensions(source_width, source_height, options.max_long_edge)?;
    let encoder = select_encoder(ffmpeg, width, height, &options, control)?;
    if control.stopping() {
        return Ok(None);
    }
    let first_frame = wait_for_first_frame(&*source, width, height, control)?;
    if control.stopping() {
        return Ok(None);
    }
    let first_frame =
        first_frame.ok_or_else(|| Failure::from("尚未收到可录制的视频帧".to_string()))?;
    let initial_sequence = if options.record_audio {
        latest_audio_sequence(&*source)?
    } else {
        0
    };
    encode_output(
        ffmpeg,
        source,
        &options,
        generation,
        control,
        events,
        &encoder,
        width,
        height,
        first_frame,
        initial_sequence,
    )
    .map(Some)
}

#[allow(clippy::too_many_arguments)]
fn encode_output(
    ffmpeg: &Path,
    source: Arc<dyn FrameSource>,
    options: &RecordingOptions,
    generation: u64,
    control: &Arc<SessionControl>,
    events: &mpsc::Sender<MediaEvent>,
    encoder: &str,
    width: u32,
    height: u32,
    first_frame: VideoFrame,
    initial_sequence: u64,
) -> Result<PathBuf, Failure> {
    let (partial, reserved) = reserve_output(&options.directory, "iPhonePlay", "partial.mp4")?;
    drop(reserved);
    let final_path = partial.with_file_name(
        partial
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace(".partial.mp4", ".mp4"),
    );
    let log_path = partial.with_extension("ffmpeg.log");
    let result = encode(
        ffmpeg,
        Arc::clone(&source),
        options,
        generation,
        control,
        events,
        encoder,
        width,
        height,
        first_frame,
        initial_sequence,
        &partial,
        &log_path,
    );
    match result {
        Ok(()) => {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&partial)
                .map_err(|e| Failure {
                    message: format!("录像封装后无法检查文件：{e}"),
                    partial: Some(partial.clone()),
                })?;
            if file
                .metadata()
                .map_err(|e| Failure {
                    message: format!("无法检查录像文件长度：{e}"),
                    partial: Some(partial.clone()),
                })?
                .len()
                < 32
            {
                return Err(Failure {
                    message: "编码器没有生成有效的 MP4 文件".into(),
                    partial: Some(partial),
                });
            }
            file.sync_all().map_err(|e| Failure {
                message: format!("录像写入磁盘失败：{e}"),
                partial: Some(partial.clone()),
            })?;
            drop(file);
            publish_file(&partial, &final_path).map_err(|message| Failure {
                message,
                partial: Some(partial),
            })?;
            Ok(final_path)
        }
        Err(message) => {
            if let Ok(mut log) = OpenOptions::new().append(true).create(true).open(&log_path) {
                let _ = writeln!(log, "\niPhonePlay: {message}");
                let _ = log.sync_all();
            }
            Err(Failure {
                message: format!(
                    "{message}。临时录像与诊断信息已保留：{}",
                    log_path.display()
                ),
                partial: Some(partial),
            })
        }
    }
}

fn quiet_command(path: &Path) -> Command {
    let mut command = Command::new(path);
    command
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    command
}

fn wait_process(
    child: &mut Child,
    timeout: Duration,
    control: &SessionControl,
) -> Result<bool, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) => {}
            Err(error) => return Err(format!("无法读取编码器状态：{error}")),
        }
        if start.elapsed() >= timeout || control.stopping() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn encoder_args(command: &mut Command, encoder: &str, bitrate_kbps: u32) {
    command.args(["-c:v", encoder, "-b:v", &format!("{bitrate_kbps}k")]);
    match encoder {
        "h264_nvenc" => {
            command.args(["-preset", "p4", "-rc", "vbr"]);
        }
        "h264_amf" => {
            command.args(["-quality", "balanced"]);
        }
        "h264_qsv" => {
            command.args(["-preset", "veryfast"]);
        }
        _ => {
            command.args(["-preset", "veryfast"]);
        }
    }
}

fn select_encoder(
    ffmpeg: &Path,
    width: u32,
    height: u32,
    options: &RecordingOptions,
    control: &SessionControl,
) -> Result<String, String> {
    // Execute a tiny encode at the actual output dimensions, not just `-encoders`:
    // compiled-in hardware encoders can still fail without a driver or adapter.
    for encoder in ["h264_nvenc", "h264_amf", "h264_qsv", "libx264"] {
        if control.stopping() {
            return Ok("libx264".into());
        }
        let mut command = quiet_command(ffmpeg);
        command.stderr(Stdio::null()).args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black:s={width}x{height}:r={}", options.fps),
            "-frames:v",
            "2",
            "-an",
            "-pix_fmt",
            "yuv420p",
        ]);
        encoder_args(&mut command, encoder, options.bitrate_kbps);
        command.args(["-f", "null", "NUL"]);
        let mut child = command
            .spawn()
            .map_err(|e| format!("无法运行 FFmpeg：{e}"))?;
        if wait_process(&mut child, Duration::from_secs(8), control)? {
            return Ok(encoder.to_string());
        }
    }
    Err("硬件编码器不可用，软件 H.264 编码器也无法启动；请检查 FFmpeg 和磁盘状态".into())
}

fn validate_frame(frame: &VideoFrame, width: u32, height: u32) -> Result<(), String> {
    let length = width as usize * height as usize * 3 / 2;
    if frame.width != width
        || frame.height != height
        || frame.stride != width
        || frame.data.len() != length
    {
        return Err("Core 返回的 NV12 画布或缓冲区尺寸不匹配".into());
    }
    Ok(())
}

fn wait_for_first_frame(
    source: &dyn FrameSource,
    width: u32,
    height: u32,
    control: &SessionControl,
) -> Result<Option<VideoFrame>, String> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) && !control.stopping() {
        if let Some(frame) = source.nv12(width, height)? {
            validate_frame(&frame, width, height)?;
            return Ok(Some(frame));
        }
        thread::sleep(Duration::from_millis(15));
    }
    Ok(None)
}

fn latest_audio_sequence(source: &dyn FrameSource) -> Result<u64, String> {
    let mut sequence = 0;
    for _ in 0..512 {
        let Some(packet) = source.audio(sequence)? else {
            break;
        };
        if packet.sequence <= sequence {
            return Err("手机音频包序号没有递增".into());
        }
        sequence = packet.sequence;
    }
    Ok(sequence)
}

enum WriterEvent {
    Connected,
    Finished(Result<(), String>),
}

#[allow(clippy::too_many_arguments)]
fn encode(
    ffmpeg: &Path,
    source: Arc<dyn FrameSource>,
    options: &RecordingOptions,
    generation: u64,
    control: &Arc<SessionControl>,
    events: &mpsc::Sender<MediaEvent>,
    encoder: &str,
    width: u32,
    height: u32,
    first_frame: VideoFrame,
    initial_sequence: u64,
    partial: &Path,
    log_path: &Path,
) -> Result<(), String> {
    let video_pipe = OutputPipe::create("video")?;
    let audio_pipe = if options.record_audio {
        Some(OutputPipe::create("audio")?)
    } else {
        None
    };
    let log = File::create(log_path).map_err(|e| format!("无法创建录制诊断文件：{e}"))?;
    let mut command = quiet_command(ffmpeg);
    command.stderr(Stdio::from(log)).args([
        "-hide_banner",
        "-loglevel",
        "warning",
        "-nostdin",
        "-thread_queue_size",
        "8",
        "-f",
        "rawvideo",
        "-pixel_format",
        "nv12",
        "-video_size",
        &format!("{width}x{height}"),
        "-framerate",
        &options.fps.to_string(),
        "-i",
        &video_pipe.path,
    ]);
    if let Some(pipe) = &audio_pipe {
        command.args([
            "-thread_queue_size",
            "64",
            "-f",
            "s16le",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-i",
            &pipe.path,
        ]);
    }
    command.args(["-map", "0:v:0"]);
    if options.record_audio {
        command.args([
            "-map", "1:a:0", "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2",
        ]);
    } else {
        command.arg("-an");
    }
    encoder_args(&mut command, encoder, options.bitrate_kbps);
    command
        .args([
            "-vf",
            "scale=in_range=full:out_range=tv",
            "-pix_fmt",
            "yuv420p",
            "-color_range",
            "tv",
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-fps_mode",
            "cfr",
            "-r",
            &options.fps.to_string(),
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            "-y",
        ])
        .arg(partial);
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动录制编码器：{e}"))?;
    let started_at = control.begin();
    let (writer_tx, writer_rx) = mpsc::channel();
    let mut writers = Vec::new();
    let video_source = Arc::clone(&source);
    let video_control = Arc::clone(control);
    let video_tx = writer_tx.clone();
    let fps = options.fps;
    writers.push(thread::spawn(move || {
        let result = video_writer(
            video_pipe,
            video_source,
            video_control,
            video_tx.clone(),
            first_frame,
            width,
            height,
            fps,
        );
        let _ = video_tx.send(WriterEvent::Finished(result));
    }));
    if let Some(pipe) = audio_pipe {
        let audio_control = Arc::clone(control);
        let audio_tx = writer_tx.clone();
        writers.push(thread::spawn(move || {
            let result = audio_writer(
                pipe,
                source,
                audio_control,
                audio_tx.clone(),
                initial_sequence,
            );
            let _ = audio_tx.send(WriterEvent::Finished(result));
        }));
    }
    drop(writer_tx);
    let expected_writers = writers.len();
    let mut connected = 0;
    let mut completed = 0;
    let mut announced = false;
    let mut finalizing_at = None;
    let result = loop {
        while let Ok(event) = writer_rx.try_recv() {
            match event {
                WriterEvent::Connected => connected += 1,
                WriterEvent::Finished(Ok(())) => completed += 1,
                WriterEvent::Finished(Err(error)) => {
                    control.abort.store(true, Ordering::Release);
                    let _ = child.kill();
                    let _ = child.wait();
                    for writer in writers {
                        let _ = writer.join();
                    }
                    return Err(error);
                }
            }
        }
        if connected == expected_writers && !announced {
            let _ = events.send(MediaEvent::Started {
                generation,
                temporary_path: partial.to_path_buf(),
                encoder: encoder.to_string(),
                started_at,
            });
            announced = true;
        }
        if control.stopping() && finalizing_at.is_none() {
            finalizing_at = Some(Instant::now());
            let _ = events.send(MediaEvent::Finalizing { generation });
        }
        if finalizing_at.is_some_and(|start: Instant| start.elapsed() >= FINALIZE_TIMEOUT) {
            break Err("录像封装超过两分钟，已停止编码器".into());
        }
        if !announced && started_at.elapsed() > Duration::from_secs(25) {
            break Err("编码器未能连接音视频输入管道".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    break Err(format!(
                        "FFmpeg 异常退出（{status}），可能是磁盘不足或编码失败"
                    ));
                }
                if !control.stopping() {
                    break Err("FFmpeg 在停止录制前提前退出".into());
                }
                // A final message may still be in transit after the last pipe closes.
                if completed == expected_writers {
                    break Ok(());
                }
            }
            Ok(None) => {}
            Err(error) => break Err(format!("无法读取 FFmpeg 退出状态：{error}")),
        }
        thread::sleep(Duration::from_millis(10));
    };
    if result.is_err() {
        control.abort.store(true, Ordering::Release);
        let _ = child.kill();
    }
    let _ = child.wait();
    for writer in writers {
        if writer.join().is_err() && result.is_ok() {
            return Err("录制写入线程意外停止".into());
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn video_writer(
    mut pipe: OutputPipe,
    source: Arc<dyn FrameSource>,
    control: Arc<SessionControl>,
    tx: mpsc::Sender<WriterEvent>,
    mut latest: VideoFrame,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<(), String> {
    pipe.connect(&control)?;
    let _ = tx.send(WriterEvent::Connected);
    let mut emitted = 0_u64;
    let mut read_error_since = None;
    while !control.aborting() {
        let stopped = control.stopping();
        let due = video_frames_due(control.elapsed(), fps, stopped).max(1);
        if stopped && emitted >= due {
            return Ok(());
        }
        if emitted >= due {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        // Retain the last decoded frame for missing frames / temporary stalls.
        // Stop/finalize must work even if the source has disconnected already.
        if !stopped {
            match source.nv12(width, height) {
                Ok(Some(frame)) => {
                    read_error_since = None;
                    validate_frame(&frame, width, height)?;
                    if frame.timestamp_100ns >= latest.timestamp_100ns {
                        latest = frame;
                    }
                }
                Ok(None) => {
                    read_error_since = None;
                }
                Err(error) => {
                    // Capture can report its native disconnect a few ticks
                    // before the app receives the corresponding event. Retain
                    // the last frame while that event requests a normal stop.
                    let since = read_error_since.get_or_insert_with(Instant::now);
                    if !control.stopping() && since.elapsed() >= Duration::from_secs(2) {
                        return Err(format!("读取录制画面失败：{error}"));
                    }
                }
            }
        }
        // Bounded work per iteration, but never discard timing debt.
        for _ in 0..(due - emitted).min(fps as u64 * 2) {
            pipe.write_all(&latest.data, &control)?;
            emitted += 1;
        }
    }
    Err("录制视频写入已中止".into())
}

fn audio_writer(
    mut pipe: OutputPipe,
    source: Arc<dyn FrameSource>,
    control: Arc<SessionControl>,
    tx: mpsc::Sender<WriterEvent>,
    mut sequence: u64,
) -> Result<(), String> {
    pipe.connect(&control)?;
    let _ = tx.send(WriterEvent::Connected);
    let mut normalizer = PcmNormalizer::default();
    let mut queue = VecDeque::new();
    let mut emitted = 0_u64;
    let mut bytes = Vec::with_capacity(480 * 4);
    let mut read_error_since = None;
    while !control.aborting() {
        let stopped = control.stopping();
        let due = units_due(control.elapsed(), 48_000);
        if stopped && emitted >= due {
            return Ok(());
        }
        // Read a bounded number of source packets. Keep no more than 100 ms of
        // pending sound; when recovering from an encoder stall, advance toward
        // live audio and fill missing clock slots with silence.
        if !stopped {
            for _ in 0..128 {
                let packet = match source.audio(sequence) {
                    Ok(packet) => {
                        read_error_since = None;
                        packet
                    }
                    Err(error) if control.stopping() => {
                        let _ = error;
                        break;
                    }
                    Err(error) => {
                        let since = read_error_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_secs(2) {
                            return Err(format!("读取手机声音失败：{error}"));
                        }
                        break;
                    }
                };
                let Some(packet) = packet else {
                    break;
                };
                if packet.sequence <= sequence {
                    return Err("手机声音包序号停滞".into());
                }
                sequence = packet.sequence;
                queue.extend(normalizer.convert(&packet)?);
                while queue.len() > 4800 {
                    queue.pop_front();
                }
            }
        }
        if emitted >= due {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        let count = (due - emitted).min(480) as usize;
        bytes.clear();
        for _ in 0..count {
            let stereo = queue.pop_front().unwrap_or([0, 0]);
            bytes.extend_from_slice(&stereo[0].to_le_bytes());
            bytes.extend_from_slice(&stereo[1].to_le_bytes());
        }
        pipe.write_all(&bytes, &control)?;
        emitted += count as u64;
    }
    Err("录制音频写入已中止".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ConstantSource;
    impl FrameSource for ConstantSource {
        fn dimensions(&self) -> Result<(u32, u32), String> {
            Ok((64, 64))
        }
        fn nv12(&self, width: u32, height: u32) -> Result<Option<VideoFrame>, String> {
            Ok(Some(VideoFrame {
                width,
                height,
                stride: width,
                timestamp_100ns: 0,
                data: vec![128; width as usize * height as usize * 3 / 2],
            }))
        }
        fn bgra(&self) -> Result<Option<super::super::BgraFrame>, String> {
            Ok(None)
        }
        fn audio(&self, _: u64) -> Result<Option<super::super::AudioPacket>, String> {
            Ok(None)
        }
    }

    fn assert_encoder_failure(ffmpeg: &Path, directory: &Path, encoder: &str) {
        let source = Arc::new(ConstantSource);
        let (tx, rx) = mpsc::channel();
        let options = RecordingOptions {
            directory: directory.to_path_buf(),
            record_audio: false,
            fps: 30,
            ..RecordingOptions::default()
        };
        let first = source.nv12(64, 64).unwrap().unwrap();
        let failure = encode_output(
            ffmpeg,
            source,
            &options,
            9,
            &Arc::new(SessionControl::new()),
            &tx,
            encoder,
            64,
            64,
            first,
            0,
        )
        .unwrap_err();
        assert!(!failure.message.is_empty());
        let partial = failure
            .partial
            .expect("failed recordings retain their diagnostic path");
        assert!(
            partial.is_file(),
            "partial file must not be deleted on failure"
        );
        assert!(partial.to_string_lossy().ends_with(".partial.mp4"));
        let final_path = PathBuf::from(partial.to_string_lossy().replace(".partial.mp4", ".mp4"));
        assert!(!final_path.exists(), "failed file must not be published");
        let log = std::fs::read_to_string(partial.with_extension("ffmpeg.log")).unwrap();
        assert!(!log.is_empty(), "preserve failure diagnostics");
        assert!(
            !rx.try_iter()
                .any(|event| matches!(event, MediaEvent::Saved { .. }))
        );
    }

    #[test]
    fn encoder_launch_failure_preserves_partial_and_diagnostics() {
        let directory = tempfile::tempdir().unwrap();
        let invalid_executable = directory.path().join("invalid-ffmpeg.exe");
        std::fs::write(
            &invalid_executable,
            b"This is deliberately not a PE executable",
        )
        .unwrap();
        assert_encoder_failure(&invalid_executable, directory.path(), "libx264");
    }

    #[test]
    #[ignore = "requires prepared FFmpeg; intentionally fails a real encoder process"]
    fn ffmpeg_encoder_error_preserves_partial_and_diagnostics() {
        let ffmpeg =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/tools/ffmpeg/ffmpeg.exe");
        assert!(ffmpeg.is_file(), "run scripts/prepare-ffmpeg.ps1 first");
        let directory = tempfile::tempdir().unwrap();
        assert_encoder_failure(
            &ffmpeg,
            directory.path(),
            "iphoneplay_nonexistent_h264_encoder",
        );
    }

    #[test]
    fn rejects_mismatched_native_canvas() {
        let frame = VideoFrame {
            width: 4,
            height: 4,
            stride: 8,
            timestamp_100ns: 0,
            data: vec![0; 24],
        };
        assert!(validate_frame(&frame, 4, 4).is_err());
    }

    #[test]
    fn missing_encoder_is_a_failure_not_saved() {
        struct NoSource;
        impl FrameSource for NoSource {
            fn dimensions(&self) -> Result<(u32, u32), String> {
                panic!("missing FFmpeg must fail first")
            }
            fn nv12(&self, _: u32, _: u32) -> Result<Option<VideoFrame>, String> {
                unreachable!()
            }
            fn bgra(&self) -> Result<Option<super::super::BgraFrame>, String> {
                unreachable!()
            }
            fn audio(&self, _: u64) -> Result<Option<super::super::AudioPacket>, String> {
                unreachable!()
            }
        }
        let (tx, rx) = mpsc::channel();
        run(
            PathBuf::from("Z:/missing-iphoneplay-ffmpeg.exe"),
            Arc::new(NoSource),
            RecordingOptions::default(),
            7,
            Arc::new(SessionControl::new()),
            tx,
        );
        assert!(matches!(
            rx.recv().unwrap(),
            MediaEvent::Failed {
                generation: 7,
                partial_path: None,
                ..
            }
        ));
    }
}
