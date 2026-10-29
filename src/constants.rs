use chrono::TimeDelta;

pub const MINUTES_IN_DAY: u16 = 24 * 60;
pub const SCROLL_OFFSET_MINS: u16 = 7 * 60;
pub const VIEWPORT_MINS: u16 = 6 * 60;
pub const ROWS_PER_HOUR: u16 = 12;
pub const RESOLUTION_IN_MINS: u16 = 5;
/// Days to display in TUI
pub const NUM_DAYS: TimeDelta = TimeDelta::days(3);
/// Determines when to the fetch new events
pub const BUFFER_DAYS: TimeDelta = TimeDelta::days(3);
pub const FETCH_DAYS: TimeDelta = TimeDelta::days(7);
pub const START_OFFSET: TimeDelta = TimeDelta::days(1);
pub const TIME_FORMAT: &str = "%Y-%m-%d %H:%M";
pub const TIME_FORMAT_24H: &str = "%Y-%m-%d %H:%M";
pub const TIME_FORMAT_12H: &str = "%Y-%m-%d %I:%M %p";
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar.events";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoogleColor {
    pub id: Option<&'static str>, // None = default calendar color
    pub name: &'static str,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub const GOOGLE_EVENT_COLORS: &[GoogleColor] = &[
    GoogleColor { id: None, name: "Default (Calendar)", r: 150, g: 150, b: 150 },
    GoogleColor { id: Some("1"), name: "Lavender", r: 121, g: 134, b: 203 },
    GoogleColor { id: Some("2"), name: "Sage", r: 51, g: 182, b: 121 },
    GoogleColor { id: Some("3"), name: "Grape", r: 142, g: 36, b: 170 },
    GoogleColor { id: Some("4"), name: "Flamingo", r: 230, g: 124, b: 115 },
    GoogleColor { id: Some("5"), name: "Banana", r: 246, g: 191, b: 38 },
    GoogleColor { id: Some("6"), name: "Tangerine", r: 244, g: 81, b: 30 },
    GoogleColor { id: Some("7"), name: "Peacock", r: 3, g: 155, b: 229 },
    GoogleColor { id: Some("8"), name: "Graphite", r: 97, g: 97, b: 97 },
    GoogleColor { id: Some("9"), name: "Blueberry", r: 63, g: 81, b: 181 },
    GoogleColor { id: Some("10"), name: "Basil", r: 11, g: 128, b: 67 },
    GoogleColor { id: Some("11"), name: "Tomato", r: 213, g: 0, b: 0 },
];
