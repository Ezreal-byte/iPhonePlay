use super::AudioPacket;
use std::time::Duration;

pub fn recording_dimensions(
    width: u32,
    height: u32,
    max_long_edge: u32,
) -> Result<(u32, u32), String> {
    if width < 2 || height < 2 || width > 16384 || height > 16384 || max_long_edge < 2 {
        return Err("当前画面尺寸无效".into());
    }
    let edge = width.max(height);
    let limit = edge.min(max_long_edge) as u64;
    let output_width = ((width as u64 * limit / edge as u64) as u32 & !1).max(2);
    let output_height = ((height as u64 * limit / edge as u64) as u32 & !1).max(2);
    Ok((output_width, output_height))
}

pub(super) fn units_due(elapsed: Duration, rate: u32) -> u64 {
    (elapsed.as_nanos().saturating_mul(rate as u128) / 1_000_000_000).min(u64::MAX as u128) as u64
}

/// A schedule retains all elapsed-time debt. A slow writer repeats the latest
/// frame until it catches up; its wall-clock duration can never silently shrink.
pub(super) fn video_frames_due(elapsed: Duration, fps: u32, stopped: bool) -> u64 {
    let whole = units_due(elapsed, fps);
    if stopped {
        // Round the final fractional frame upward, within one frame of stop.
        whole + u64::from(!(elapsed.as_nanos() * fps as u128).is_multiple_of(1_000_000_000))
    } else {
        whole + 1
    }
}

#[derive(Default)]
pub(super) struct PcmNormalizer {
    sample_rate: u32,
    channels: u16,
    remainder: u64,
}

impl PcmNormalizer {
    pub(super) fn convert(&mut self, packet: &AudioPacket) -> Result<Vec<[i16; 2]>, String> {
        if !(8000..=192_000).contains(&packet.sample_rate)
            || !(1..=8).contains(&packet.channels)
            || packet.bits_per_sample != 16
        {
            return Err("手机音频格式不受支持，需要 16 位 PCM".into());
        }
        let block = packet.channels as usize * 2;
        if packet.data.is_empty() || !packet.data.len().is_multiple_of(block) {
            return Err("手机音频包大小无效".into());
        }
        if self.sample_rate != packet.sample_rate || self.channels != packet.channels {
            self.sample_rate = packet.sample_rate;
            self.channels = packet.channels;
            self.remainder = 0;
        }
        let frames = packet.data.len() / block;
        let scaled = frames as u64 * 48_000 + self.remainder;
        let target_frames = (scaled / packet.sample_rate as u64) as usize;
        self.remainder = scaled % packet.sample_rate as u64;
        let read = |frame: usize, channel: usize| {
            let channel = if packet.channels == 1 { 0 } else { channel };
            let start = frame * block + channel * 2;
            i16::from_le_bytes([packet.data[start], packet.data[start + 1]]) as f64
        };
        let mut output = Vec::with_capacity(target_frames);
        for target in 0..target_frames {
            let position = if target_frames <= 1 || frames <= 1 {
                0.0
            } else {
                target as f64 * (frames - 1) as f64 / (target_frames - 1) as f64
            };
            let lower = position as usize;
            let upper = (lower + 1).min(frames - 1);
            let fraction = position - lower as f64;
            let mut stereo = [0; 2];
            for (channel, value) in stereo.iter_mut().enumerate() {
                let low = read(lower, channel);
                let high = read(upper, channel);
                *value = (low + (high - low) * fraction)
                    .round()
                    .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            }
            output.push(stereo);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_is_even_and_never_enlarges_source() {
        assert_eq!(recording_dimensions(1179, 2556, 1920).unwrap(), (884, 1920));
        assert_eq!(recording_dimensions(640, 480, 1920).unwrap(), (640, 480));
        assert_eq!(recording_dimensions(2556, 1179, 1920).unwrap(), (1920, 884));
        assert!(recording_dimensions(0, 0, 1920).is_err());
    }

    #[test]
    fn slow_encoder_retains_elapsed_time_debt() {
        assert_eq!(video_frames_due(Duration::from_secs(10), 60, false), 601);
        let emitted = 420;
        assert_eq!(
            video_frames_due(Duration::from_secs(10), 60, true) - emitted,
            180
        );
        assert_eq!(video_frames_due(Duration::from_millis(1001), 30, true), 31);
        assert_eq!(units_due(Duration::from_secs(1800), 48_000), 86_400_000);
    }

    #[test]
    fn resampler_remainder_prevents_packet_rounding_drift() {
        let mut normalizer = PcmNormalizer::default();
        let packet = AudioPacket {
            sequence: 1,
            sample_rate: 44_100,
            channels: 1,
            bits_per_sample: 16,
            data: vec![0; 100 * 2],
        };
        let frames: usize = (0..441)
            .map(|_| normalizer.convert(&packet).unwrap().len())
            .sum();
        assert_eq!(frames, 48_000);
    }

    #[test]
    fn mono_is_mapped_to_both_channels() {
        let packet = AudioPacket {
            sequence: 1,
            sample_rate: 48_000,
            channels: 1,
            bits_per_sample: 16,
            data: vec![0x34, 0x12, 0x00, 0x80],
        };
        assert_eq!(
            PcmNormalizer::default().convert(&packet).unwrap(),
            [[0x1234, 0x1234], [-32768, -32768]]
        );
    }
}
