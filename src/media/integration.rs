//! Explicit opt-in FFmpeg integration tests. These use synthetic owned frames,
//! not iPhone hardware, and must never be reported as device acceptance tests.
use super::*;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

struct SyntheticSource {
    started: Instant,
}

impl FrameSource for SyntheticSource {
    fn dimensions(&self) -> Result<(u32, u32), String> {
        Ok((320, 480))
    }

    fn nv12(&self, width: u32, height: u32) -> Result<Option<VideoFrame>, String> {
        let luma_size = width as usize * height as usize;
        let mut data = vec![128; luma_size * 3 / 2];
        let elapsed = self.started.elapsed();
        let phase = (elapsed.as_millis() / 33) as usize;
        for (position, value) in data[..luma_size].iter_mut().enumerate() {
            *value = ((position % width as usize + phase) % 220 + 16) as u8;
        }
        Ok(Some(VideoFrame {
            width,
            height,
            stride: width,
            timestamp_100ns: (elapsed.as_nanos() / 100) as i64,
            data,
        }))
    }

    fn bgra(&self) -> Result<Option<BgraFrame>, String> {
        Ok(Some(BgraFrame {
            width: 2,
            height: 2,
            stride: 8,
            data: vec![
                0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255,
            ],
        }))
    }

    fn audio(&self, after_sequence: u64) -> Result<Option<AudioPacket>, String> {
        let available = (self.started.elapsed().as_millis() / 10) as u64;
        if after_sequence >= available {
            return Ok(None);
        }
        let sequence = after_sequence + 1;
        let mut data = Vec::with_capacity(480 * 4);
        for sample in 0..480 {
            let position = (sequence - 1) * 480 + sample;
            let phase = position as f64 * 440.0 * std::f64::consts::TAU / 48000.0;
            let value = (phase.sin() * 5000.0) as i16;
            data.extend_from_slice(&value.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        }
        Ok(Some(AudioPacket {
            sequence,
            sample_rate: 48000,
            channels: 2,
            bits_per_sample: 16,
            data,
        }))
    }
}

#[test]
#[ignore = "requires prepared runtime/tools/ffmpeg/ffmpeg.exe; synthetic encoder round-trip"]
fn ffmpeg_records_video_and_phone_audio_then_decodes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ffmpeg = root.join("runtime/tools/ffmpeg/ffmpeg.exe");
    assert!(ffmpeg.is_file(), "run scripts/prepare-ffmpeg.ps1 first");
    let directory = root
        .join("build/media-verification")
        .join(unique_stem("synthetic"));
    fs::create_dir_all(&directory).unwrap();
    for record_audio in [true, false] {
        let mut recorder = Recorder::new(ffmpeg.clone());
        let source = Arc::new(SyntheticSource {
            started: Instant::now(),
        });
        let options = RecordingOptions {
            directory: directory.clone(),
            max_long_edge: 1920,
            fps: 30,
            record_audio,
            bitrate_kbps: 2000,
        };
        recorder.start(source.clone(), options, 42).unwrap();
        let startup = Instant::now();
        let started = loop {
            if let Some(event) = recorder.try_event() {
                match event {
                    MediaEvent::Started {
                        started_at,
                        encoder,
                        ..
                    } => {
                        eprintln!("synthetic recording: audio={record_audio}, encoder={encoder}");
                        break started_at;
                    }
                    MediaEvent::Failed { message, .. } => panic!("{message}"),
                    _ => {}
                }
            }
            assert!(
                startup.elapsed() < Duration::from_secs(45),
                "encoder startup timed out"
            );
            thread::sleep(Duration::from_millis(10));
        };
        while started.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(10));
        }
        recorder.stop();
        let finalizing = Instant::now();
        let saved = loop {
            if let Some(event) = recorder.try_event() {
                match event {
                    MediaEvent::Saved { path, .. } => break path,
                    MediaEvent::Failed { message, .. } => panic!("{message}"),
                    _ => {}
                }
            }
            assert!(
                finalizing.elapsed() < Duration::from_secs(20),
                "finalization timed out"
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert!(saved.is_file());
        assert!(!saved.to_string_lossy().contains(".partial."));
        let decoded = Command::new(&ffmpeg)
            .creation_flags(0x0800_0000)
            .args(["-v", "error", "-i"])
            .arg(&saved)
            .args(["-f", "null", "NUL"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        eprintln!("synthetic round-trip passed: {}", saved.display());
        let screenshot = save_png(&source.bgra().unwrap().unwrap(), &directory).unwrap();
        let decoder = png::Decoder::new(std::io::BufReader::new(File::open(&screenshot).unwrap()));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (2, 2));
    }
}

#[test]
fn output_directory_that_is_a_file_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("not-a-directory");
    fs::write(&file, b"existing user file").unwrap();
    assert!(reserve_output(&file, "test", "mp4").is_err());
    assert_eq!(fs::read(file).unwrap(), b"existing user file");
}
