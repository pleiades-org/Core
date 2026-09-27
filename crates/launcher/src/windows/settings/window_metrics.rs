use std::fmt;

/// Corner radius in 96-DPI pixels. Zero gives square corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CornerRadius(u8);

impl CornerRadius {
    pub const MAX: u8 = 32;
    const DEFAULT: u8 = 16;

    pub fn new(value: u32) -> Self {
        Self(value.min(u32::from(Self::MAX)) as u8)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        parse_in_range(text, u32::from(Self::MAX), "Corner rounding").map(Self::new)
    }

    pub fn logical(self) -> i32 {
        i32::from(self.0)
    }
}

impl Default for CornerRadius {
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}

impl fmt::Display for CornerRadius {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Gap between Core and the physical screen edge in 96-DPI pixels.
/// Zero keeps the automatic behaviour: Core stays inside the work area, clear of the taskbar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdgeSpacing(u16);

impl EdgeSpacing {
    pub const MAX: u16 = 200;

    pub fn new(value: u32) -> Self {
        Self(value.min(u32::from(Self::MAX)) as u16)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        parse_in_range(text, u32::from(Self::MAX), "Screen edge spacing").map(Self::new)
    }

    pub fn logical(self) -> i32 {
        i32::from(self.0)
    }
}

impl fmt::Display for EdgeSpacing {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

fn parse_in_range(text: &str, max: u32, name: &str) -> Result<u32, String> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= max)
        .ok_or_else(|| format!("{name} must be a whole number from 0 to {max}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_parse_within_range_and_clamp_programmatic_values() {
        assert_eq!(CornerRadius::parse(" 0 "), Ok(CornerRadius::new(0)));
        assert_eq!(CornerRadius::parse("32").map(CornerRadius::logical), Ok(32));
        assert_eq!(EdgeSpacing::parse("200").map(EdgeSpacing::logical), Ok(200));
        for text in ["", "-1", "33", "1.5", "x"] {
            assert!(CornerRadius::parse(text).is_err(), "{text}");
        }
        assert!(EdgeSpacing::parse("201").is_err());
        assert_eq!(CornerRadius::new(999).logical(), 32);
        assert_eq!(EdgeSpacing::new(999).logical(), 200);
        assert_eq!(CornerRadius::default().logical(), 16);
        assert_eq!(EdgeSpacing::default().logical(), 0);
    }
}
