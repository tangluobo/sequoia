//! Shared encoder presets. Explicit legacy size/rate settings always take precedence.
use tomato_core::Connection;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoQuality {
    Ultra,
    #[default]
    High,
    Native,
    Fast,
}
impl VideoQuality {
    pub const ALL: [Self; 4] = [Self::High, Self::Ultra, Self::Native, Self::Fast];
    pub fn from_connection(connection: &Connection) -> Self {
        match connection.text("adbVideoQuality") {
            "ultra" => Self::Ultra,
            "high" => Self::High,
            "native" => Self::Native,
            "fast" => Self::Fast,
            _ => Self::High,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Ultra => "超清 2560",
            Self::High => "高清 1920",
            Self::Native => "原生分辨率",
            Self::Fast => "流畅 1024",
        }
    }
    fn settings(self) -> (&'static str, u64, u64) {
        match self {
            Self::Ultra => ("ultra", 2560, 24_000_000),
            Self::High => ("high", 1920, 24_000_000),
            Self::Native => ("native", 0, 32_000_000),
            Self::Fast => ("fast", 1024, 4_000_000),
        }
    }
    pub fn apply(self, connection: &mut Connection) {
        let (name, size, rate) = self.settings();
        connection.set("adbVideoQuality", name);
        connection.set("adbVideoMaxSize", size);
        connection.set("adbVideoBitRate", rate);
    }
    pub(super) fn encoder_settings(connection: &Connection) -> (u64, u64) {
        let (_, default_size, default_rate) = Self::from_connection(connection).settings();
        let size = match connection.number("adbVideoMaxSize") {
            0 => default_size,
            size => size.min(4096),
        };
        let rate = match connection.number("adbVideoBitRate") {
            0 => default_rate,
            rate => rate.clamp(1_000_000, 32_000_000),
        };
        (size, rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_and_explicit_legacy_encoder_settings() {
        let mut c = Connection::new("ADB");
        assert_eq!(VideoQuality::encoder_settings(&c), (1920, 24_000_000));
        for quality in VideoQuality::ALL {
            quality.apply(&mut c);
            let (_, size, rate) = quality.settings();
            assert_eq!(VideoQuality::from_connection(&c), quality);
            assert_eq!(VideoQuality::encoder_settings(&c), (size, rate));
        }
        c.set("adbVideoQuality", "high");
        c.set("adbVideoMaxSize", 1440);
        c.set("adbVideoBitRate", 8_000_000);
        assert_eq!(VideoQuality::encoder_settings(&c), (1440, 8_000_000));
        c.set("adbVideoMaxSize", 99999);
        c.set("adbVideoBitRate", 999_999_999);
        assert_eq!(VideoQuality::encoder_settings(&c), (4096, 32_000_000));
    }
}
