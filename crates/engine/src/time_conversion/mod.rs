mod parse_time;

pub use parse_time::{parse_date, parse_time, recognizes_time};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeZone {
    Eastern,
    Central,
    Mountain,
    Pacific,
    London,
    Berlin,
    Sydney,
    Tokyo,
    India,
    Utc,
    Est,
    Edt,
    Cst,
    Cdt,
    Mst,
    Mdt,
    Pst,
    Pdt,
    Bst,
}

impl TimeZone {
    pub fn label(self) -> &'static str {
        match self {
            Self::Eastern => "ET",
            Self::Central => "CT",
            Self::Mountain => "MT",
            Self::Pacific => "PT",
            Self::London => "UK",
            Self::Berlin => "Berlin",
            Self::Sydney => "Sydney",
            Self::Tokyo => "Tokyo",
            Self::India => "India",
            Self::Utc => "UTC",
            Self::Est => "EST",
            Self::Edt => "EDT",
            Self::Cst => "CST",
            Self::Cdt => "CDT",
            Self::Mst => "MST",
            Self::Mdt => "MDT",
            Self::Pst => "PST",
            Self::Pdt => "PDT",
            Self::Bst => "BST",
        }
    }

    /// Explicit seasonal abbreviations are fixed offsets, unlike regional ET/PT/UK.
    pub fn fixed_minutes(self) -> Option<i32> {
        match self {
            Self::Utc => Some(0),
            Self::Est | Self::Cdt => Some(-300),
            Self::Edt => Some(-240),
            Self::Cst | Self::Mdt => Some(-360),
            Self::Mst | Self::Pdt => Some(-420),
            Self::Pst => Some(-480),
            Self::Bst => Some(60),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u16,
    pub day: u16,
}

impl std::fmt::Display for Date {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTime {
    pub hour: u16,
    pub minute: u16,
}

impl std::fmt::Display for ClockTime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hour = match self.hour % 12 {
            0 => 12,
            hour => hour,
        };
        let period = if self.hour < 12 { "am" } else { "pm" };
        write!(formatter, "{hour}:{:02} {period}", self.minute)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRequest {
    pub time: ClockTime,
    pub source: TimeZone,
    pub destination: TimeZone,
    pub date: Option<Date>,
}

#[derive(Debug)]
pub struct ConvertedTime {
    pub source_date: Date,
    pub destination_date: Date,
    pub time: ClockTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeError {
    Syntax,
    InvalidTime,
    InvalidDate,
    UnknownZone,
    Unavailable,
    Nonexistent,
    Ambiguous,
}

impl TimeError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Syntax => "Try 9pm ET to UK · optionally add on YYYY-MM-DD",
            Self::InvalidTime => "Use a time such as 9pm, 9:30 pm or 21:30",
            Self::InvalidDate => "Use a valid date from 1900–2100: on YYYY-MM-DD",
            Self::UnknownZone => "Unknown zone · try ET, CT, MT, PT, UK, UTC or a supported city",
            Self::Unavailable => "Windows time-zone rules are unavailable",
            Self::Nonexistent => "That local time is skipped when daylight saving starts",
            Self::Ambiguous => {
                "That time occurs twice · use an explicit offset abbreviation, e.g. EDT or EST"
            }
        }
    }
}

/// Platform adapters supply current dates and daylight-saving rules without UI dependencies.
pub trait TimeConverter: Send {
    fn convert(&mut self, request: TimeRequest) -> Result<ConvertedTime, TimeError>;
}
