#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureState {
    #[default]
    Preparing,
    Waiting,
    Connecting,
    Streaming,
    Interrupted,
    Failed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RecordingState {
    #[default]
    Idle,
    Starting,
    Recording,
    Finalizing,
    Completed,
    Failed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    AirPlay,
    Usb,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Welcome,
    Usb,
    Workspace,
    Settings,
    Recovery,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Switch(Source),
    Disconnect,
    Exit,
}

#[derive(Debug, Default)]
pub struct SessionGate {
    generation: u64,
    active_device: Option<String>,
}
impl SessionGate {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
    pub fn reserve(&mut self, device: String) -> Option<u64> {
        if self.active_device.is_some() {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.active_device = Some(device);
        Some(self.generation)
    }
    pub fn invalidate(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.active_device = None;
        self.generation
    }
    pub fn occupied(&self) -> bool {
        self.active_device.is_some()
    }
}

pub fn fit_aspect(available: [f32; 2], source: [u32; 2]) -> [f32; 2] {
    if source.contains(&0) || available.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return [0.0, 0.0];
    }
    let scale = (available[0] / source[0] as f32).min(available[1] / source[1] as f32);
    [source[0] as f32 * scale, source[1] as f32 * scale]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn additional_device_cannot_steal_session() {
        let mut gate = SessionGate::default();
        let gen1 = gate.reserve("phone1".into()).unwrap();
        assert!(gate.reserve("phone2".into()).is_none());
        gate.invalidate();
        assert!(!gate.accepts(gen1));
        let gen2 = gate.reserve("phone2".into()).unwrap();
        assert_ne!(gen1, gen2);
    }
    #[test]
    fn preview_fits_without_cropping_or_bezel() {
        assert_eq!(fit_aspect([800., 600.], [100, 200]), [300., 600.]);
        assert_eq!(fit_aspect([800., 600.], [200, 100]), [800., 400.]);
        assert_eq!(fit_aspect([800., 600.], [0, 0]), [0., 0.]);
    }
}
