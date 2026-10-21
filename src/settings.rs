use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}
impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统",
            Self::Light => "明亮",
            Self::Dark => "暗黑",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub theme: Theme,
    pub scale: f32,
    pub reduced_motion: bool,
    pub close_to_tray: bool,
    pub tray_notice_seen: bool,
    pub remember_window: bool,
    pub window_size: [f32; 2],
    pub preview_long_edge: u32,
    pub preview_fps: u32,
    pub preview_rounded: bool,
    pub preview_corner_radius: f32,
    pub floating_always_on_top: bool,
    pub play_audio: bool,
    pub volume: f32,
    pub mouse_sensitivity: f32,
    pub wheel_sensitivity: f32,
    pub bound_client: Option<String>,
    pub screenshot_dir: PathBuf,
    pub recording_dir: PathBuf,
    pub recording_long_edge: u32,
    pub recording_fps: u32,
    pub recording_bitrate: u32,
    pub record_audio: bool,
    pub receiver_name: String,
    pub usb_mode: u32,
}

impl Default for Settings {
    fn default() -> Self {
        let dirs = directories::UserDirs::new();
        let fallback = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let pictures = dirs
            .as_ref()
            .and_then(|d| d.picture_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| fallback.join("Pictures"));
        let videos = dirs
            .as_ref()
            .and_then(|d| d.video_dir())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| fallback.join("Videos"));
        Self {
            schema_version: SCHEMA_VERSION,
            theme: Theme::System,
            scale: 1.0,
            reduced_motion: false,
            close_to_tray: true,
            tray_notice_seen: false,
            remember_window: true,
            window_size: [1280.0, 900.0],
            preview_long_edge: 1920,
            preview_fps: 60,
            preview_rounded: false,
            preview_corner_radius: 16.,
            floating_always_on_top: true,
            play_audio: true,
            volume: 1.0,
            mouse_sensitivity: 1.0,
            wheel_sensitivity: 1.0,
            bound_client: None,
            screenshot_dir: pictures.join("iPhonePlay"),
            recording_dir: videos.join("iPhonePlay"),
            recording_long_edge: 1920,
            recording_fps: 60,
            recording_bitrate: 12000,
            record_audio: true,
            receiver_name: format!(
                "iPhonePlay · {}",
                std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows".into())
            ),
            usb_mode: 0,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.to_string()),
        };
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("配置文件无法读取：{e}"))?;
        if value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            > u64::from(SCHEMA_VERSION)
        {
            return Err("配置来自更高版本，未覆盖原文件".into());
        }
        let mut s: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        s.validate();
        Ok(s)
    }
    pub fn validate(&mut self) {
        self.scale = if self.scale.is_finite() {
            self.scale.clamp(0.75, 2.0)
        } else {
            1.0
        };
        self.volume = self.volume.clamp(0.0, 1.0);
        self.mouse_sensitivity = self.mouse_sensitivity.clamp(0.1, 4.0);
        self.wheel_sensitivity = self.wheel_sensitivity.clamp(0.1, 4.0);
        self.preview_fps = self.preview_fps.clamp(15, 60);
        self.preview_corner_radius = if self.preview_corner_radius.is_finite() {
            self.preview_corner_radius.clamp(8., 32.)
        } else {
            16.
        };
        self.preview_long_edge = self.preview_long_edge.clamp(640, 3840);
        self.recording_fps = if self.recording_fps == 30 { 30 } else { 60 };
        self.recording_long_edge = self.recording_long_edge.clamp(640, 3840);
        self.recording_bitrate = self.recording_bitrate.clamp(2000, 50000);
        self.usb_mode = self.usb_mode.min(2);
        if self.receiver_name.trim().is_empty() {
            self.receiver_name = Self::default().receiver_name;
        }
        self.receiver_name = self.receiver_name.chars().take(80).collect();
        for (value, min) in self.window_size.iter_mut().zip([960.0, 640.0]) {
            if !value.is_finite() {
                *value = min;
            }
            *value = value.clamp(min, 7680.0);
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        atomic_replace(&tmp, path).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
fn atomic_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };
    let a: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(a.as_ptr()),
            PCWSTR(b.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::other)
}
#[cfg(not(windows))]
fn atomic_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("iPhonePlay")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_roundtrip_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings {
            receiver_name: "iPhonePlay · 测试电脑".into(),
            ..Default::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        s.theme = Theme::Dark;
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        assert!(!path.with_extension("json.tmp").exists());
    }
    #[test]
    fn rejects_future_schema_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, b"{\"schema_version\":999}").unwrap();
        assert!(Settings::load(&p).is_err());
        assert!(fs::read_to_string(p).unwrap().contains("999"));
    }
    #[test]
    fn validates_untrusted_settings() {
        let mut s = Settings {
            scale: f32::NAN,
            recording_fps: 999,
            receiver_name: " ".into(),
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.scale, 1.0);
        assert_eq!(s.recording_fps, 60);
        assert!(!s.receiver_name.trim().is_empty());
    }
}
