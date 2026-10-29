use crate::constants::{
    BUFFER_DAYS, FETCH_DAYS, GOOGLE_EVENT_COLORS, MINUTES_IN_DAY, NUM_DAYS,
    RESOLUTION_IN_MINS, SCROLL_OFFSET_MINS, START_OFFSET, TIME_FORMAT_12H, TIME_FORMAT_24H,
    VIEWPORT_MINS,
};
use crate::event::{AppEvent, Event, EventHandler, EventsFetched};
use crate::{Calendar, Config, Result};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeDelta, Timelike, Utc};
use google_calendar3::api::{CalendarListEntry, Event as CEvent};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Block;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
};
use ratatui_textarea::TextArea;

pub fn parse_time_str(s: &str) -> Option<NaiveDateTime> {
    let trimmed = s.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, TIME_FORMAT_24H) {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, TIME_FORMAT_12H) {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %I:%M%p") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %I:%M:%S %p") {
        return Some(dt);
    }
    None
}

pub fn safe_local_datetime(naive: chrono::NaiveDateTime) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    match chrono::Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => {
            chrono::Local.from_local_datetime(&(naive + chrono::Duration::hours(1)))
                .single()
                .unwrap_or_else(chrono::Local::now)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CalendarInfo {
    pub display_name: String,
    pub id: String,
    pub color_bg: Option<String>,
    pub color_fg: Option<String>,
}

impl TryFrom<CalendarListEntry> for CalendarInfo {
    type Error = ();

    fn try_from(value: CalendarListEntry) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            display_name: value.summary.unwrap_or_else(|| "Untitled".to_string()),
            id: value.id.ok_or(())?,
            color_bg: value.background_color,
            color_fg: value.foreground_color,
        })
    }
}

#[derive(Debug, Clone)]
pub struct EventNode {
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub cal_info: CalendarInfo,
    pub color_id: Option<String>,
    pub is_all_day: bool,
}

impl EventNode {
    pub fn spans_date(&self, date: NaiveDate) -> bool {
        let start_date = self.start_time.with_timezone(&Local).date_naive();
        let end_date = (self.end_time.with_timezone(&Local) - TimeDelta::seconds(1)).date_naive();
        date >= start_date && date <= end_date
    }

    pub fn get_color(&self) -> ratatui::style::Color {
        // 1. If event has specific Google Calendar color_id (1-11)
        if let Some(ref cid) = self.color_id {
            match cid.as_str() {
                "1" => return Color::Rgb(121, 134, 203), // Lavender
                "2" => return Color::Rgb(51, 182, 121),  // Sage
                "3" => return Color::Rgb(142, 36, 170),  // Grape
                "4" => return Color::Rgb(230, 124, 115), // Flamingo
                "5" => return Color::Rgb(246, 191, 38),  // Banana
                "6" => return Color::Rgb(244, 81, 30),   // Tangerine
                "7" => return Color::Rgb(3, 155, 229),   // Peacock
                "8" => return Color::Rgb(97, 97, 97),    // Graphite
                "9" => return Color::Rgb(63, 81, 181),   // Blueberry
                "10" => return Color::Rgb(11, 128, 67),  // Basil
                "11" => return Color::Rgb(213, 0, 0),    // Tomato
                _ => {}
            }
        }

        // 2. Fall back to calendar background color
        if let Some(ref hex) = self.cal_info.color_bg {
            let hex_clean = hex.trim().trim_start_matches('#');
            if hex_clean.len() == 6
                && let (Ok(r), Ok(g), Ok(b)) = (
                    u8::from_str_radix(&hex_clean[0..2], 16),
                    u8::from_str_radix(&hex_clean[2..4], 16),
                    u8::from_str_radix(&hex_clean[4..6], 16),
                )
            {
                return Color::Rgb(r, g, b);
            }
        }

        // 3. Fallback color: primary defaults to Cyan; others get a distinct deterministic palette color
        if self.cal_info.id == "primary" || self.cal_info.id.is_empty() {
            Color::Cyan
        } else {
            let hash = self.cal_info.id.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
            let colors = [
                Color::Rgb(66, 133, 244),   // Google Blue
                Color::Rgb(11, 128, 67),    // Basil Green
                Color::Rgb(142, 36, 170),   // Grape Purple
                Color::Rgb(244, 81, 30),    // Tangerine Orange
                Color::Rgb(3, 155, 229),    // Peacock Cyan
                Color::Rgb(230, 124, 115),  // Flamingo Pink
                Color::Rgb(246, 191, 38),   // Banana Yellow
                Color::Rgb(63, 81, 181),    // Blueberry
            ];
            colors[(hash as usize) % colors.len()]
        }
    }
}

impl TryFrom<(String, CEvent)> for EventNode {
    type Error = ();

    fn try_from((cal_id, cal_event): (String, CEvent)) -> std::result::Result<Self, Self::Error> {
        let mut node = EventNode::try_from(cal_event)?;
        node.cal_info.id = cal_id;
        Ok(node)
    }
}

impl TryFrom<CEvent> for EventNode {
    type Error = ();

    fn try_from(cal_event: CEvent) -> std::result::Result<Self, Self::Error> {
        let id = cal_event.id.ok_or(())?;

        let is_all_day = cal_event
            .start
            .as_ref()
            .map(|d| d.date.is_some() && d.date_time.is_none())
            .unwrap_or(false);

        let event_start_datetime = cal_event.start.and_then(|d| {
            d.date_time.or_else(|| {
                d.date
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .map(|naive| safe_local_datetime(naive).with_timezone(&Utc))
            })
        });
        let event_end_datetime = cal_event.end.and_then(|d| {
            d.date_time.or_else(|| {
                d.date
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .map(|naive| safe_local_datetime(naive).with_timezone(&Utc))
            })
        });

        let (Some(event_start_datetime), Some(event_end_datetime)) = (event_start_datetime, event_end_datetime) else {
            return Err(());
        };

        let is_all_day = is_all_day || (event_end_datetime - event_start_datetime).num_hours() >= 24;

        let summary = cal_event.summary.unwrap_or_else(|| "Untitled".to_string());
        let cal_info = match cal_event.organizer {
            Some(organizer) => CalendarInfo {
                display_name: organizer.display_name.unwrap_or_else(|| "Untitled".to_string()),
                id: organizer.email.unwrap_or_else(|| "primary".to_string()),
                color_bg: None,
                color_fg: None,
            },
            None => CalendarInfo {
                display_name: "Untitled".to_string(),
                id: "primary".to_string(),
                color_bg: None,
                color_fg: None,
            },
        };

        Ok(EventNode {
            id,
            summary,
            description: cal_event.description,
            start_time: event_start_datetime,
            end_time: event_end_datetime,
            cal_info,
            color_id: cal_event.color_id,
            is_all_day,
        })
    }
}

#[derive(Default, PartialEq)]
pub enum AppMode {
    #[default]
    Normal,
    Insert,
    InsertEdit,
    Visual,
    VisualEdit,
    DeleteConfirm,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum ActiveField {
    #[default]
    Summary,
    Description,
    StartTime,
    EndTime,
    Calendar,
    Color,
}

#[derive(Debug, Default)]
pub struct EventPopup<'a> {
    pub summary: TextArea<'a>,
    pub description: TextArea<'a>,
    pub start_time: TextArea<'a>,
    pub end_time: TextArea<'a>,
    pub selected_color_id: Option<String>,
    pub color_dropdown_open: bool,
    pub active_field: ActiveField,
}

#[derive(Debug)]
pub struct InsertEvent {
    pub start_time: DateTime<Utc>,
    pub duration: TimeDelta,
    pub color_id: Option<String>,
}

impl InsertEvent {
    pub fn get_color(&self) -> Color {
        if let Some(ref cid) = self.color_id {
            match cid.as_str() {
                "1" => Color::Rgb(121, 134, 203), // Lavender
                "2" => Color::Rgb(51, 182, 121),  // Sage
                "3" => Color::Rgb(142, 36, 170),  // Grape
                "4" => Color::Rgb(230, 124, 115), // Flamingo
                "5" => Color::Rgb(246, 191, 38),  // Banana
                "6" => Color::Rgb(244, 81, 30),   // Tangerine
                "7" => Color::Rgb(3, 155, 229),   // Peacock
                "8" => Color::Rgb(97, 97, 97),    // Graphite
                "9" => Color::Rgb(63, 81, 181),   // Blueberry
                "10" => Color::Rgb(11, 128, 67),  // Basil
                "11" => Color::Rgb(213, 0, 0),    // Tomato
                _ => Color::Yellow,
            }
        } else {
            Color::Yellow
        }
    }
}

pub struct App {
    pub running: bool,
    pub events: EventHandler,

    pub mode: AppMode,

    // View events
    pub start_date: NaiveDate,
    pub scroll_offset: u16,
    pub viewport_mins: u16,
    pub cal_event_nodes: Vec<EventNode>,
    pub num_days: TimeDelta,
    pub now: DateTime<Local>,
    pub is_now_timeline_visible: bool,

    pub sel_event_id: Option<String>,
    pub sel_cal_ind: Option<usize>,

    // Load events
    pub cal: Calendar,
    pub loaded_start: NaiveDate,
    pub loaded_end: NaiveDate,
    pub cal_infos: Vec<CalendarInfo>,

    // Insert event
    pub insert_event: InsertEvent,
    pub popup: EventPopup<'static>,

    // Track background network status independently
    pub is_fetching: bool,
    pub pending_key: Option<char>,
    pub show_help: bool,
    pub last_deleted: Option<EventNode>,
    pub last_scroll: Option<(std::time::Instant, bool)>,
    pub scroll_streak: u16,
    pub last_insert_scroll: Option<(std::time::Instant, bool)>,
    pub insert_scroll_streak: u16,
    pub last_auto_refresh: std::time::Instant,
    pub use_24h_clock: bool,
}

impl App {
    /// Constructs a new instance of [`App`].
    pub async fn new() -> Result<Self> {
        let config = Config::new()?;
        let use_24h_clock = config.use_24h_clock.unwrap_or(true);

        let cal = Calendar::new(config).await?;

        let now = Local::now();
        let start_date = now.date_naive() - START_OFFSET;

        let rounded_minute = (now.minute() / RESOLUTION_IN_MINS as u32) * RESOLUTION_IN_MINS as u32;
        let rounded_time = now
            .with_minute(rounded_minute)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap()
            .with_timezone(&Utc);

        let insert_event = InsertEvent {
            start_time: rounded_time,
            duration: TimeDelta::minutes(RESOLUTION_IN_MINS as i64 * 2),
            color_id: None,
        };

        let mut app = App {
            running: true,
            mode: Default::default(),
            scroll_offset: SCROLL_OFFSET_MINS,
            viewport_mins: VIEWPORT_MINS,
            events: Default::default(),
            cal_event_nodes: Default::default(),
            sel_event_id: Default::default(),
            sel_cal_ind: Default::default(),
            start_date,
            num_days: NUM_DAYS,
            cal,
            now,
            loaded_start: start_date,
            loaded_end: start_date,
            is_now_timeline_visible: true,
            insert_event,
            popup: Default::default(),
            is_fetching: false,
            cal_infos: Default::default(),
            pending_key: None,
            show_help: false,
            last_deleted: None,
            last_scroll: None,
            scroll_streak: 0,
            last_insert_scroll: None,
            insert_scroll_streak: 0,
            last_auto_refresh: std::time::Instant::now(),
            use_24h_clock,
        };

        app.initial_fetch_events(
            start_date - BUFFER_DAYS,
            start_date + app.num_days + BUFFER_DAYS,
        )
        .await;

        app.switch_active_field(Default::default());

        Ok(app)
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        if let Ok(size) = terminal.size() {
            self.update_viewport_from_height(size.height);
        }

        self.jump_to_current_time();

        while self.running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
            match self.events.next().await? {
                Event::Tick => self.tick(),
                Event::Crossterm(event) => match event {
                    CrosstermEvent::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                        self.handle_key_events(key_event)?
                    }
                    CrosstermEvent::Resize(_width, height) => {
                        terminal.autoresize()?;
                        self.update_viewport_from_height(height);
                    }
                    _ => {}
                },
                Event::App(app_event) => match app_event {
                    AppEvent::Quit => self.quit(),
                    AppEvent::EventFetched(events_fetched) => {
                        self.handle_fetched_events(events_fetched)
                    }
                    AppEvent::FetchFailed(_) => self.is_fetching = false,
                    AppEvent::EventCreated(event_node) => self.handle_event_created(event_node),
                    AppEvent::EventReload(events_fetched) => {
                        self.handle_reload_events(events_fetched)
                    }
                    AppEvent::EventUpdated(event_node) => self.handle_event_created(event_node),
                    AppEvent::EventDeleted(event_id) => self.handle_event_deleted(event_id),
                    AppEvent::CalendarFetched(cal_infos) => self.handle_fetched_cals(cal_infos),
                },
            }
        }
        Ok(())
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&mut self) {
        let now = Local::now();
        self.now = now;

        // Routine background sync every 30 seconds
        const AUTO_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
        if self.last_auto_refresh.elapsed() >= AUTO_REFRESH_INTERVAL {
            self.last_auto_refresh = std::time::Instant::now();
            if matches!(self.mode, AppMode::Normal | AppMode::Insert | AppMode::Visual) {
                self.refresh_events();
            }
        }
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Handles the key events and updates the state of [`App`].
    pub fn handle_key_events(&mut self, key_event: KeyEvent) -> Result<()> {
        match self.mode {
            AppMode::Normal => self.handle_normal_key_events(key_event),
            AppMode::Insert => self.handle_insert_key_events(key_event),
            AppMode::InsertEdit => self.handle_insert_edit_key_events(key_event),
            AppMode::Visual => self.handle_visual_key_events(key_event),
            AppMode::VisualEdit => self.handle_visual_edit_key_events(key_event),
            AppMode::DeleteConfirm => self.handle_delete_confirm_key_events(key_event),
        }
    }

    /// Handle normal key events
    pub fn handle_normal_key_events(&mut self, key_event: KeyEvent) -> Result<()> {
        if self.show_help {
            if matches!(key_event.code, KeyCode::Char('?' | 'q') | KeyCode::Esc) {
                self.show_help = false;
            }
            return Ok(());
        }

        // Handle multi-key sequences: gg, GG, rn, m[1-4]
        if let Some(prev) = self.pending_key.take() {
            match (prev, key_event.code) {
                ('g', KeyCode::Char('g')) => {
                    self.scroll_to_top();
                    return Ok(());
                }
                ('g', KeyCode::Char('G')) | ('G', KeyCode::Char('G')) => {
                    self.scroll_to_bottom();
                    return Ok(());
                }
                ('r', KeyCode::Char('n')) => {
                    self.jump_to_current_time();
                    return Ok(());
                }
                ('m', KeyCode::Char('1' | 'd' | 'D')) => {
                    self.create_macro_event("Deep Work", TimeDelta::minutes(60));
                    return Ok(());
                }
                ('m', KeyCode::Char('2' | 's' | 'S')) => {
                    self.create_macro_event("Standup", TimeDelta::minutes(15));
                    return Ok(());
                }
                ('m', KeyCode::Char('3' | 'l' | 'L')) => {
                    self.create_macro_event("Lunch", TimeDelta::minutes(45));
                    return Ok(());
                }
                ('m', KeyCode::Char('4' | 'p' | 'P')) => {
                    self.create_macro_event("Planning", TimeDelta::minutes(30));
                    return Ok(());
                }
                _ => {}
            }
        }

        match key_event.code {
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Char('q') => self.events.send(AppEvent::Quit),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit)
            }
            // Toggle 12h / 24h clock format
            KeyCode::Char('C') if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_time_format();
            }
            // Scroll vertically
            KeyCode::Char('k') | KeyCode::Up => self.scroll_up(),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_down(),
            KeyCode::Char('K') => self.big_scroll_up(),
            KeyCode::Char('J') => self.big_scroll_down(),

            // Page scrolling: Ctrl-d / Ctrl-u (half page), Ctrl-f / Ctrl-b (full page)
            KeyCode::Char('d') if key_event.modifiers == KeyModifiers::CONTROL => {
                let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
                self.scroll_offset = (self.scroll_offset + self.viewport_mins / 2).min(max_offset);
            }
            KeyCode::Char('u') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_mins / 2);
            }
            KeyCode::Char('f') if key_event.modifiers == KeyModifiers::CONTROL => {
                let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
                self.scroll_offset = (self.scroll_offset + self.viewport_mins).min(max_offset);
            }
            KeyCode::Char('b') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewport_mins);
            }

            // Top / Bottom
            KeyCode::Char('g') => {
                self.pending_key = Some('g');
            }
            KeyCode::Char('G') => {
                self.scroll_to_bottom();
            }

            // Right now (rn) or refresh (R)
            KeyCode::Char('r') => {
                self.pending_key = Some('r');
            }
            KeyCode::Char('R') => {
                self.refresh_events();
            }

            // Scroll horizontally
            KeyCode::Char('h') | KeyCode::Left => self.scroll_left(),
            KeyCode::Char('l') | KeyCode::Right => self.scroll_right(),

            // Jump to current time
            KeyCode::Char('t') => self.jump_to_current_time(),

            // Toggle
            KeyCode::Char('T') => self.is_now_timeline_visible = !self.is_now_timeline_visible,

            // Cycle calendar
            KeyCode::Char('c') => {
                self.select_next_cal();
            }

            // Undo last deletion (plain 'u' with no modifiers)
            KeyCode::Char('u') if key_event.modifiers.is_empty() => {
                if let Some(deleted) = self.last_deleted.take() {
                    self.cal_event_nodes.push(deleted.clone());
                    self.cal_event_nodes.sort_by_key(|e| e.start_time);
                    self.create_event(deleted);
                }
            }

            // Change mode
            KeyCode::Char('i') => self.mode = AppMode::Insert,
            KeyCode::Char('v') => {
                self.mode = AppMode::Visual;
                if self.sel_event_id.is_none() {
                    self.select_first_visible_current_day_event();
                }
            }

            // View mode switching: Day ('d'), 3-Day ('3' or 'D'), Week ('w')
            KeyCode::Char('d') if key_event.modifiers.is_empty() => {
                self.set_view_days(1);
            }
            KeyCode::Char('w') => {
                self.set_view_days(7);
            }
            KeyCode::Char('3') | KeyCode::Char('D') => {
                self.set_view_days(3);
            }

            // Macro prefix ('m')
            KeyCode::Char('m') => {
                self.pending_key = Some('m');
            }

            // Quick 1-Key Time-Blocking Macros
            KeyCode::Char('1') => {
                self.create_macro_event("Deep Work", TimeDelta::minutes(60));
            }
            KeyCode::Char('2') => {
                self.create_macro_event("Standup", TimeDelta::minutes(15));
            }
            KeyCode::Char('4') => {
                self.create_macro_event("Planning", TimeDelta::minutes(30));
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle insert key events
    pub fn handle_insert_key_events(&mut self, key_event: KeyEvent) -> Result<()> {
        if let Some(prev) = self.pending_key.take()
            && prev == 'r' && key_event.code == KeyCode::Char('n')
        {
            self.move_insert_current_time();
            return Ok(());
        }

        match key_event.code {
            KeyCode::Esc => self.mode = AppMode::Normal,
            KeyCode::Char('r') => {
                self.pending_key = Some('r');
            }
            KeyCode::Char('t') => self.move_insert_current_time(),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit)
            }
            // Cycle color in insert mode
            KeyCode::Char('c') => self.cycle_insert_color(),
            // Toggle 12h / 24h clock format
            KeyCode::Char('C') if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_time_format();
            }
            // Scroll vertically
            KeyCode::Char('k') | KeyCode::Up => self.move_insert_up(),
            KeyCode::Char('j') | KeyCode::Down => self.move_insert_down(),

            // Scroll horizontally
            KeyCode::Char('h') | KeyCode::Left => self.move_insert_left(),
            KeyCode::Char('l') | KeyCode::Right => self.move_insert_right(),

            // Extend
            KeyCode::Char('K') => self.extend_insert_up(),
            KeyCode::Char('J') => self.extend_insert_down(),
            KeyCode::Char('H') => self.extend_insert_left(),
            KeyCode::Char('L') => self.extend_insert_right(),

            // Insert event details
            KeyCode::Enter => self.enter_insert_event_details(),

            // Toggle
            KeyCode::Char('T') => self.is_now_timeline_visible = !self.is_now_timeline_visible,
            _ => {}
        }
        Ok(())
    }

    fn handle_popup_events(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Tab => {
                let active_field = match self.popup.active_field {
                    ActiveField::Summary => ActiveField::Description,
                    ActiveField::Description => ActiveField::StartTime,
                    ActiveField::StartTime => ActiveField::EndTime,
                    ActiveField::EndTime => ActiveField::Calendar,
                    ActiveField::Calendar => ActiveField::Color,
                    ActiveField::Color => ActiveField::Summary,
                };
                self.popup.color_dropdown_open = false;
                self.switch_active_field(active_field);
            }
            KeyCode::BackTab => {
                let active_field = match self.popup.active_field {
                    ActiveField::Summary => ActiveField::Color,
                    ActiveField::Description => ActiveField::Summary,
                    ActiveField::StartTime => ActiveField::Description,
                    ActiveField::EndTime => ActiveField::StartTime,
                    ActiveField::Calendar => ActiveField::EndTime,
                    ActiveField::Color => ActiveField::Calendar,
                };
                self.popup.color_dropdown_open = false;
                self.switch_active_field(active_field);
            }
            KeyCode::Char('h') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.select_prev_cal();
            }
            KeyCode::Char('l') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.select_next_cal();
            }
            _ => {}
        }
    }

    /// Handle insertedit key events
    pub fn handle_insert_edit_key_events(&mut self, key_event: KeyEvent) -> Result<()> {
        if self.popup.color_dropdown_open {
            match key_event.code {
                KeyCode::Esc => {
                    self.popup.color_dropdown_open = false;
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.select_prev_color();
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.select_next_color();
                    return Ok(());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.popup.color_dropdown_open = false;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }

        match key_event.code {
            KeyCode::Esc => self.mode = AppMode::Insert,
            KeyCode::Enter if self.popup.active_field != ActiveField::Description => {
                self.submit_popup()?;
            }
            KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_popup()?;
            }
            KeyCode::Tab | KeyCode::BackTab => self.handle_popup_events(key_event),
            KeyCode::Char('h' | 'l') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.handle_popup_events(key_event);
            }
            KeyCode::Char('s' | 'S') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.submit_popup()?
            }
            _ => {
                if self.popup.active_field == ActiveField::Calendar {
                    match key_event.code {
                        KeyCode::Char('h') | KeyCode::Left => self.select_prev_cal(),
                        KeyCode::Char('l') | KeyCode::Right => self.select_next_cal(),
                        _ => {}
                    }
                    return Ok(());
                }
                if self.popup.active_field == ActiveField::Color {
                    match key_event.code {
                        KeyCode::Char('h') | KeyCode::Left => self.select_prev_color(),
                        KeyCode::Char('l') | KeyCode::Right => self.select_next_color(),
                        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char(' ') => {
                            self.popup.color_dropdown_open = true;
                        }
                        _ => {}
                    }
                    return Ok(());
                }
                let active_ta = match self.popup.active_field {
                    ActiveField::Summary => &mut self.popup.summary,
                    ActiveField::Description => &mut self.popup.description,
                    ActiveField::StartTime => &mut self.popup.start_time,
                    ActiveField::EndTime => &mut self.popup.end_time,
                    ActiveField::Calendar | ActiveField::Color => return Ok(()),
                };
                active_ta.input(key_event);
            }
        }
        Ok(())
    }

    /// Handle visual key events
    pub fn handle_visual_key_events(&mut self, key_event: KeyEvent) -> Result<()> {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('v') => {
                self.mode = Default::default();
                self.sel_event_id = None;
            }
            KeyCode::Char('q') => self.events.send(AppEvent::Quit),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.events.send(AppEvent::Quit)
            }
            // Toggle 12h / 24h clock format
            KeyCode::Char('C') if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_time_format();
            }
            // move through events chronologically
            KeyCode::Char('k') | KeyCode::Up => self.select_prev_event(),
            KeyCode::Char('j') | KeyCode::Down => self.select_next_event(),

            // jump days
            KeyCode::Char('h') | KeyCode::Left => self.select_prev_day_event(),
            KeyCode::Char('l') | KeyCode::Right => self.select_next_day_event(),

            // Shift event / duration
            KeyCode::Char('J') => {
                // extend duration +5m
                if let Some(idx) = self.get_selected_event_index() {
                    let mut node = self.cal_event_nodes[idx].clone();
                    node.end_time += TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
                    self.patch_event(node);
                }
            }
            KeyCode::Char('K') => {
                // shrink duration -5m, min 5m
                if let Some(idx) = self.get_selected_event_index() {
                    let mut node = self.cal_event_nodes[idx].clone();
                    let new_end = node.end_time - TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
                    if new_end > node.start_time {
                        node.end_time = new_end;
                        self.patch_event(node);
                    }
                }
            }
            // Move event start/end +5m / -5m
            KeyCode::Char('+') | KeyCode::Char('=') => {
                if let Some(idx) = self.get_selected_event_index() {
                    let mut node = self.cal_event_nodes[idx].clone();
                    let shift = TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
                    node.start_time += shift;
                    node.end_time += shift;
                    self.patch_event(node);
                }
            }
            KeyCode::Char('-') => {
                if let Some(idx) = self.get_selected_event_index() {
                    let mut node = self.cal_event_nodes[idx].clone();
                    let shift = TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
                    node.start_time -= shift;
                    node.end_time -= shift;
                    self.patch_event(node);
                }
            }

            // Open the selected event details
            KeyCode::Enter => self.enter_visual_event_details(),

            // Open delete popup
            KeyCode::Char('d') => {
                if self.sel_event_id.is_some() {
                    self.mode = AppMode::DeleteConfirm;
                }
            }

            // Toggle
            KeyCode::Char('T') => self.is_now_timeline_visible = !self.is_now_timeline_visible,
            _ => {}
        }
        Ok(())
    }

    /// Handle visualedit key events
    pub fn handle_visual_edit_key_events(&mut self, key_event: KeyEvent) -> Result<()> {
        if self.popup.color_dropdown_open {
            match key_event.code {
                KeyCode::Esc => {
                    self.popup.color_dropdown_open = false;
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.select_prev_color();
                    return Ok(());
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.select_next_color();
                    return Ok(());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.popup.color_dropdown_open = false;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }

        match key_event.code {
            KeyCode::Esc => self.mode = AppMode::Visual,
            KeyCode::Enter if self.popup.active_field != ActiveField::Description => {
                self.submit_popup()?;
            }
            KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.submit_popup()?;
            }
            KeyCode::Char('h' | 'l') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.handle_popup_events(key_event);
            }
            KeyCode::Tab | KeyCode::BackTab => self.handle_popup_events(key_event),
            KeyCode::Char('s' | 'S') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.submit_popup()?
            }
            _ => {
                if self.popup.active_field == ActiveField::Calendar {
                    match key_event.code {
                        KeyCode::Char('h') | KeyCode::Left => self.select_prev_cal(),
                        KeyCode::Char('l') | KeyCode::Right => self.select_next_cal(),
                        _ => {}
                    }
                    return Ok(());
                }
                if self.popup.active_field == ActiveField::Color {
                    match key_event.code {
                        KeyCode::Char('h') | KeyCode::Left => self.select_prev_color(),
                        KeyCode::Char('l') | KeyCode::Right => self.select_next_color(),
                        KeyCode::Down | KeyCode::Char('j') | KeyCode::Char(' ') => {
                            self.popup.color_dropdown_open = true;
                        }
                        _ => {}
                    }
                    return Ok(());
                }
                let active_ta = match self.popup.active_field {
                    ActiveField::Summary => &mut self.popup.summary,
                    ActiveField::Description => &mut self.popup.description,
                    ActiveField::StartTime => &mut self.popup.start_time,
                    ActiveField::EndTime => &mut self.popup.end_time,
                    ActiveField::Calendar | ActiveField::Color => return Ok(()),
                };
                active_ta.input(key_event);
            }
        }
        Ok(())
    }

    /// Handle visualedit key events
    pub fn handle_delete_confirm_key_events(&mut self, key_event: KeyEvent) -> Result<()> {
        match key_event.code {
            KeyCode::Esc => self.mode = AppMode::Visual,
            KeyCode::Enter => {
                // It should be only possible to enter delete confirm mode from visual mode
                if let Some(event_node) = self.get_selected_event() {
                    self.delete_event(event_node.clone());
                }
                self.mode = AppMode::Visual;
            }
            _ => {}
        }
        Ok(())
    }

    /// Routinely refresh events from Google Calendar for the currently visible range
    pub fn refresh_events(&mut self) {
        if self.is_fetching {
            return;
        }
        let start = self.loaded_start.min(self.start_date - BUFFER_DAYS);
        let end = self.loaded_end.max(self.start_date + self.num_days + BUFFER_DAYS);
        self.fetch_events(start, end, true);
    }

    /// Trigger background fetch for new events
    pub fn fetch_events(&mut self, start_date: NaiveDate, end_date: NaiveDate, is_refresh: bool) {
        if self.is_fetching {
            return;
        }
        self.is_fetching = true;

        let cal_clone = self.cal.clone();
        let sender = self.events.sender.clone();

        tokio::spawn(async move {
            match cal_clone.get_events(start_date, end_date).await {
                Ok(events) => {
                    let event_nodes = events
                        .into_iter()
                        .filter_map(|e| e.try_into().ok())
                        .collect();
                    let events_fetched = EventsFetched {
                        event_nodes,
                        start_date,
                        end_date,
                    };
                    let app_event = if is_refresh {
                        AppEvent::EventReload(events_fetched)
                    } else {
                        AppEvent::EventFetched(events_fetched)
                    };
                    let _ = sender.send(Event::App(app_event));
                }
                Err(e) => {
                    let _ = sender.send(Event::App(AppEvent::FetchFailed(e.to_string())));
                }
            }
        });
    }

    /// Enrich events with their matching calendar info (name, background color, foreground color)
    pub fn enrich_events_slice(cal_infos: &[CalendarInfo], events: &mut [EventNode]) {
        for event in events.iter_mut() {
            if let Some(matching_info) = cal_infos.iter().find(|c| c.id == event.cal_info.id) {
                event.cal_info.display_name = matching_info.display_name.clone();
                if matching_info.color_bg.is_some() {
                    event.cal_info.color_bg = matching_info.color_bg.clone();
                }
                if matching_info.color_fg.is_some() {
                    event.cal_info.color_fg = matching_info.color_fg.clone();
                }
            }
        }
    }

    pub fn enrich_events(&mut self) {
        Self::enrich_events_slice(&self.cal_infos, &mut self.cal_event_nodes);
    }

    /// Trigger intial background fetch for new events
    pub async fn initial_fetch_events(&mut self, start_date: NaiveDate, end_date: NaiveDate) {
        // Fetch calendars first so calendar_ids and colors are known immediately
        if let Ok(cals) = self.cal.get_calendars_info().await {
            let cal_infos: Vec<CalendarInfo> = cals.into_iter().filter_map(|c| c.try_into().ok()).collect();
            for info in &cal_infos {
                if !self.cal.calendar_ids.contains(&info.id) {
                    self.cal.calendar_ids.push(info.id.clone());
                }
            }
            self.cal_infos = cal_infos;
        }

        let events = match self.cal.get_events(start_date, end_date).await {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut event_nodes: Vec<EventNode> = events
            .into_iter()
            .filter_map(|e| e.try_into().ok())
            .collect();
        Self::enrich_events_slice(&self.cal_infos, &mut event_nodes);
        let events_fetched = EventsFetched {
            event_nodes,
            start_date,
            end_date,
        };
        self.handle_reload_events(events_fetched);

        // Keep background calendar discovery active
        self.is_fetching = true;
        let cal_clone = self.cal.clone();
        let sender = self.events.sender.clone();

        tokio::spawn(async move {
            match cal_clone.get_calendars_info().await {
                Ok(cal) => {
                    let cal_infos = cal.into_iter().filter_map(|c| c.try_into().ok()).collect();
                    let _ = sender.send(Event::App(AppEvent::CalendarFetched(cal_infos)));
                }
                Err(e) => {
                    let _ = sender.send(Event::App(AppEvent::FetchFailed(e.to_string())));
                }
            }
        });
    }

    /// Trigger background request for creating event
    pub fn create_event(&mut self, event_node: EventNode) {
        let cal_clone = self.cal.clone();
        let sender = self.events.sender.clone();

        tokio::spawn(async move {
            match cal_clone.create_event(event_node).await {
                Ok(event) => {
                    let Ok(event_node) = EventNode::try_from(event) else {
                        let _ = sender.send(Event::App(AppEvent::FetchFailed(
                            "Failed to convert to eventnode".to_string(),
                        )));
                        return;
                    };
                    let _ = sender.send(Event::App(AppEvent::EventCreated(event_node)));
                }
                Err(e) => {
                    let _ = sender.send(Event::App(AppEvent::FetchFailed(e.to_string())));
                }
            }
        });
    }

    /// Trigger background request for patching event
    pub fn patch_event(&mut self, event_node: EventNode) {
        let cal_clone = self.cal.clone();
        let sender = self.events.sender.clone();

        tokio::spawn(async move {
            match cal_clone.patch_event(event_node).await {
                Ok(event) => {
                    let Ok(event_node) = EventNode::try_from(event) else {
                        let _ = sender.send(Event::App(AppEvent::FetchFailed(
                            "Failed to convert to eventnode".to_string(),
                        )));
                        return;
                    };
                    let _ = sender.send(Event::App(AppEvent::EventUpdated(event_node)));
                }
                Err(e) => {
                    let _ = sender.send(Event::App(AppEvent::FetchFailed(e.to_string())));
                }
            }
        });
    }

    /// Trigger background request for deleting event
    pub fn delete_event(&mut self, event_node: EventNode) {
        self.last_deleted = Some(event_node.clone());
        let cal_clone = self.cal.clone();
        let sender = self.events.sender.clone();

        tokio::spawn(async move {
            let event_id = event_node.id.clone();
            match cal_clone.delete_event(event_node).await {
                Ok(_) => {
                    let _ = sender.send(Event::App(AppEvent::EventDeleted(event_id)));
                }
                Err(e) => {
                    let _ = sender.send(Event::App(AppEvent::FetchFailed(e.to_string())));
                }
            }
        });
    }

    fn handle_fetched_cals(&mut self, cal_infos: Vec<CalendarInfo>) {
        let mut new_ids = Vec::new();
        for info in &cal_infos {
            if !self.cal.calendar_ids.contains(&info.id) {
                new_ids.push(info.id.clone());
            }
        }
        self.cal_infos = cal_infos;
        self.is_fetching = false;

        // Enrich all currently loaded event nodes with their calendar display name and colors
        self.enrich_events();

        if !new_ids.is_empty() {
            for id in new_ids {
                self.cal.calendar_ids.push(id);
            }
            self.fetch_events(self.loaded_start, self.loaded_end, false);
        }
    }

    /// Add events
    fn handle_fetched_events(&mut self, mut events_fetched: EventsFetched) {
        Self::enrich_events_slice(&self.cal_infos, &mut events_fetched.event_nodes);
        self.cal_event_nodes.append(&mut events_fetched.event_nodes);
        self.cal_event_nodes.sort_by_key(|e| e.start_time);

        self.loaded_start = self.loaded_start.min(events_fetched.start_date);
        self.loaded_end = self.loaded_end.max(events_fetched.end_date);

        self.is_fetching = false;
    }
    fn handle_event_created(&mut self, event_node: EventNode) {
        // Since we did an optimistic update, we can replace the latest temporary/optimistic event or match by summary/start_time if needed, or simply ensure no duplicate.
        // Let's replace the last added event or update if an ID matches or if there's a temporary one.
        if let Some(pos) = self.cal_event_nodes.iter().position(|e| e.id == event_node.id) {
            self.cal_event_nodes[pos] = event_node;
        } else {
            // If ID was empty/temporary, find one with matching summary and start time or push
            if let Some(pos) = self.cal_event_nodes.iter().position(|e| e.id.is_empty() && e.summary == event_node.summary && e.start_time == event_node.start_time) {
                self.cal_event_nodes[pos] = event_node;
            } else {
                self.cal_event_nodes.push(event_node);
            }
        }
        self.cal_event_nodes.sort_by_key(|e| e.start_time);
        self.is_fetching = false;
    }

    /// Update events
    fn handle_reload_events(&mut self, mut events_fetched: EventsFetched) {
        Self::enrich_events_slice(&self.cal_infos, &mut events_fetched.event_nodes);
        self.cal_event_nodes = events_fetched.event_nodes;
        self.cal_event_nodes.sort_by_key(|e| e.start_time);

        self.loaded_start = events_fetched.start_date;
        self.loaded_end = events_fetched.end_date;

        self.is_fetching = false;
    }

    /// delete event
    fn handle_event_deleted(&mut self, event_id: String) {
        if let Some(pos) = self.cal_event_nodes.iter().position(|e| e.id == *event_id) {
            self.cal_event_nodes.remove(pos);
        }
        self.sel_event_id = None;
        self.is_fetching = false;
    }

    fn get_scroll_step(&mut self, is_down: bool) -> u16 {
        let now = std::time::Instant::now();
        const THRESHOLD: std::time::Duration = std::time::Duration::from_millis(250);

        if let Some((last_time, last_dir)) = self.last_scroll {
            if last_dir == is_down && now.duration_since(last_time) < THRESHOLD {
                self.scroll_streak = self.scroll_streak.saturating_add(1);
            } else {
                self.scroll_streak = 0;
            }
        } else {
            self.scroll_streak = 0;
        }

        self.last_scroll = Some((now, is_down));

        // Acceleration curve:
        // streak 0: 15 mins (smooth, responsive single tap)
        // streak 1..3: 30 mins
        // streak 4..7: 60 mins (1 hour)
        // streak 8..12: 120 mins (2 hours)
        // streak 13+: 180 mins (3 hours)
        match self.scroll_streak {
            0 => 15,
            1..=3 => 30,
            4..=7 => 60,
            8..=12 => 120,
            _ => 180,
        }
    }

    fn get_insert_scroll_step(&mut self, is_down: bool) -> i64 {
        let now = std::time::Instant::now();
        const THRESHOLD: std::time::Duration = std::time::Duration::from_millis(250);

        if let Some((last_time, last_dir)) = self.last_insert_scroll {
            if last_dir == is_down && now.duration_since(last_time) < THRESHOLD {
                self.insert_scroll_streak = self.insert_scroll_streak.saturating_add(1);
            } else {
                self.insert_scroll_streak = 0;
            }
        } else {
            self.insert_scroll_streak = 0;
        }

        self.last_insert_scroll = Some((now, is_down));

        // Precision 5m for taps; accelerates to 15m/30m when holding down
        match self.insert_scroll_streak {
            0..=4 => RESOLUTION_IN_MINS as i64,
            5..=10 => 15,
            _ => 30,
        }
    }

    /// Scroll vertically
    fn scroll_up(&mut self) {
        let step = self.get_scroll_step(false);
        self.scroll_offset = self.scroll_offset.saturating_sub(step);
    }
    fn scroll_down(&mut self) {
        let step = self.get_scroll_step(true);
        let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
        self.scroll_offset = (self.scroll_offset + step).min(max_offset);
    }
    fn big_scroll_up(&mut self) {
        let step = 120; // 2 hours
        self.scroll_offset = self.scroll_offset.saturating_sub(step);
    }
    fn big_scroll_down(&mut self) {
        let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
        let step = 120; // 2 hours
        self.scroll_offset = (self.scroll_offset + step).min(max_offset);
    }
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }
    pub fn scroll_to_bottom(&mut self) {
        let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
        self.scroll_offset = max_offset;
    }

    /// Scroll horizontally
    fn scroll_left(&mut self) {
        self.start_date -= TimeDelta::days(1);
        self.check_pagination();
    }
    fn scroll_right(&mut self) {
        self.start_date += TimeDelta::days(1);
        self.check_pagination();
    }

    /// Move Insert Event
    fn move_insert_up(&mut self) {
        let mins = self.get_insert_scroll_step(false);
        self.insert_event.start_time -= TimeDelta::minutes(mins);
        self.sync_viewport_to_cursor();
    }
    fn move_insert_down(&mut self) {
        let mins = self.get_insert_scroll_step(true);
        self.insert_event.start_time += TimeDelta::minutes(mins);
        self.sync_viewport_to_cursor();
    }
    fn move_insert_left(&mut self) {
        self.insert_event.start_time -= TimeDelta::days(1);
        self.sync_viewport_to_cursor();
    }
    fn move_insert_right(&mut self) {
        self.insert_event.start_time += TimeDelta::days(1);
        self.sync_viewport_to_cursor();
    }

    /// Extend Insert Event
    fn extend_insert_up(&mut self) {
        let delta = TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
        if self.insert_event.duration > delta {
            self.insert_event.duration -= delta;
        }
        self.sync_viewport_to_cursor();
    }
    fn extend_insert_down(&mut self) {
        self.insert_event.duration += TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
        self.sync_viewport_to_cursor();
    }
    fn extend_insert_left(&mut self) {
        let delta = TimeDelta::days(1);
        let min_duration = TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
        if self.insert_event.duration - delta >= min_duration {
            self.insert_event.duration -= delta;
        } else {
            self.insert_event.duration = min_duration;
        }
        self.sync_viewport_to_cursor();
    }
    fn extend_insert_right(&mut self) {
        let delta = TimeDelta::days(1);
        self.insert_event.duration += delta;
    }

    /// Select Visual Events
    fn get_selected_event_index(&self) -> Option<usize> {
        if let Some(id) = &self.sel_event_id {
            self.cal_event_nodes.iter().position(|e| e.id == *id)
        } else {
            None
        }
    }
    pub fn get_selected_event(&self) -> Option<&EventNode> {
        if let Some(id) = &self.sel_event_id {
            self.cal_event_nodes.iter().find(|e| e.id == *id)
        } else {
            None
        }
    }
    fn select_next_event(&mut self) {
        if self.cal_event_nodes.is_empty() {
            return;
        }

        let next_idx = match self.get_selected_event_index() {
            Some(idx) => (idx + 1).min(self.cal_event_nodes.len() - 1),
            None => 0,
        };

        self.sel_event_id = self.cal_event_nodes.get(next_idx).map(|e| e.id.clone());
        self.sync_viewport_to_selected_event();
    }
    fn select_prev_event(&mut self) {
        if self.cal_event_nodes.is_empty() {
            return;
        }
        let prev_idx = match self.get_selected_event_index() {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        };

        self.sel_event_id = self.cal_event_nodes.get(prev_idx).map(|e| e.id.clone());
        self.sync_viewport_to_selected_event();
    }
    fn select_next_day_event(&mut self) {
        if self.cal_event_nodes.is_empty() {
            return;
        }

        let current_idx = self.get_selected_event_index().unwrap_or(0);
        let current_date = self.cal_event_nodes[current_idx]
            .start_time
            .with_timezone(&Local)
            .date_naive();
        let target_idx = self
            .cal_event_nodes
            .iter()
            .position(|e| e.start_time.with_timezone(&Local).date_naive() > current_date)
            .unwrap_or(self.cal_event_nodes.len() - 1);

        self.sel_event_id = self.cal_event_nodes.get(target_idx).map(|e| e.id.clone());
        self.sync_viewport_to_selected_event();
    }
    fn select_prev_day_event(&mut self) {
        if self.cal_event_nodes.is_empty() {
            return;
        }

        let current_idx = self.get_selected_event_index().unwrap_or(0);
        let current_date = self.cal_event_nodes[current_idx]
            .start_time
            .with_timezone(&Local)
            .date_naive();
        let target_idx = self
            .cal_event_nodes
            .iter()
            .rposition(|e| e.start_time.with_timezone(&Local).date_naive() < current_date)
            .unwrap_or(0);

        self.sel_event_id = self.cal_event_nodes.get(target_idx).map(|e| e.id.clone());
        self.sync_viewport_to_selected_event();
    }

    /// Submit Insert Event
    fn submit_popup(&mut self) -> Result<()> {
        let start_text = self.popup.start_time.lines().join("");
        let end_text = self.popup.end_time.lines().join("");
        let start_res = parse_time_str(&start_text);
        let end_res = parse_time_str(&end_text);

        let (start_time, end_time) = match (start_res, end_res) {
            (Some(start_naive), Some(end_naive)) => (
                safe_local_datetime(start_naive).with_timezone(&Utc),
                safe_local_datetime(end_naive).with_timezone(&Utc),
            ),
            _ => (
                self.insert_event.start_time,
                self.insert_event.start_time + self.insert_event.duration,
            ),
        };

        let summary = {
            let s = self.popup.summary.lines().join(" ");
            let trimmed = s.trim();
            if trimmed.is_empty() {
                "Time Block".to_string()
            } else {
                trimmed.to_string()
            }
        };

        let description = {
            let description = self.popup.description.lines().join("\n");
            let trimmed = description.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        };

        let (id, existing_cal_info, color_id, is_all_day) = match self.mode {
            AppMode::InsertEdit => (
                Default::default(),
                self.cal_infos
                    .get(self.get_selected_cal_index())
                    .cloned()
                    .unwrap_or_else(|| CalendarInfo {
                        display_name: "Primary".to_string(),
                        id: "primary".to_string(),
                        color_bg: None,
                        color_fg: None,
                    }),
                self.popup.selected_color_id.clone(),
                false,
            ),
            AppMode::VisualEdit => {
                let Some(idx) = self.get_selected_event_index() else {
                    return Ok(());
                };
                let Some(node) = self.cal_event_nodes.get(idx) else {
                    return Ok(());
                };
                (
                    node.id.clone(),
                    node.cal_info.clone(),
                    self.popup.selected_color_id.clone(),
                    node.is_all_day,
                )
            }
            _ => return Ok(()),
        };

        let cal_info = if let Some(idx) = self.sel_cal_ind {
            self.cal_infos.get(idx).cloned().unwrap_or(existing_cal_info)
        } else {
            existing_cal_info
        };

        let event_node = EventNode {
            id,
            summary,
            description,
            start_time,
            end_time,
            cal_info,
            color_id,
            is_all_day,
        };

        match self.mode {
            AppMode::InsertEdit => {
                self.cal_event_nodes.push(event_node.clone());
                self.cal_event_nodes.sort_by_key(|e| e.start_time);
                self.mode = AppMode::Normal;
                self.create_event(event_node);
            }
            AppMode::VisualEdit => {
                if let Some(pos) = self.cal_event_nodes.iter().position(|e| e.id == event_node.id) {
                    self.cal_event_nodes[pos] = event_node.clone();
                }
                self.mode = AppMode::Visual;
                self.patch_event(event_node);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn create_macro_event(&mut self, summary: &str, duration: TimeDelta) {
        let Some(cal_info) = self.cal_infos.first().cloned().or_else(|| Some(CalendarInfo { display_name: "Primary".to_string(), id: "primary".to_string(), color_bg: None, color_fg: None })) else {
            return;
        };

        let start_time = self.insert_event.start_time;
        let end_time = start_time + duration;

        let event_node = EventNode {
            id: Default::default(),
            summary: summary.to_string(),
            description: None,
            start_time,
            end_time,
            cal_info,
            color_id: None,
            is_all_day: false,
        };

        self.cal_event_nodes.push(event_node.clone());
        self.cal_event_nodes.sort_by_key(|e| e.start_time);
        self.create_event(event_node);
    }

    // Calendar
    pub fn get_selected_cal_index(&self) -> usize {
        let AppMode::VisualEdit = self.mode else {
            return 0;
        };

        self.sel_cal_ind.unwrap_or_default()
    }
    fn select_prev_cal(&mut self) {
        if self.cal_infos.is_empty() {
            return;
        }
        let cal_len = self.cal_infos.len();
        let curr_idx = self.get_selected_cal_index();
        let prev_idx = (curr_idx + cal_len - 1) % cal_len;
        self.sel_cal_ind = Some(prev_idx);
    }
    fn select_next_cal(&mut self) {
        if self.cal_infos.is_empty() {
            return;
        }
        let cal_len = self.cal_infos.len();
        let curr_idx = self.get_selected_cal_index();
        let next_idx = (curr_idx + 1) % cal_len;
        self.sel_cal_ind = Some(next_idx);
    }

    /// Pagination
    fn check_pagination(&mut self) {
        if self.is_fetching {
            return;
        }

        if self.start_date + self.num_days + BUFFER_DAYS >= self.loaded_end {
            self.fetch_events(self.loaded_end, self.loaded_end + FETCH_DAYS, false);
        } else if self.start_date - BUFFER_DAYS <= self.loaded_start {
            self.fetch_events(self.loaded_start - FETCH_DAYS, self.loaded_start, false);
        }
    }

    /// Centers the viewport to the given time
    fn jump_to_time(&mut self, time: &DateTime<Local>) {
        self.start_date = time.date_naive() - START_OFFSET;

        let current_mins = time.hour() * 60 + time.minute();
        let half_viewport = self.viewport_mins / 2;
        let target_offset = (current_mins as u16).saturating_sub(half_viewport);
        let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);

        self.scroll_offset = target_offset.min(max_offset);
    }

    /// Jumps the calendar to Today
    fn jump_to_current_time(&mut self) {
        let now = Local::now();
        self.now = now;
        self.jump_to_time(&now);
    }

    /// Switch calendar view between Day (1), 3-Day (3), and Week (7)
    pub fn set_view_days(&mut self, days: i64) {
        use chrono::Datelike;
        self.num_days = TimeDelta::days(days);
        if days == 1 {
            self.start_date = self.now.date_naive();
        } else if days == 3 {
            self.start_date = self.now.date_naive() - TimeDelta::days(1);
        } else if days == 7 {
            let weekday = self.now.weekday().num_days_from_monday();
            self.start_date = self.now.date_naive() - TimeDelta::days(weekday as i64);
        }
        self.check_pagination();
    }

    /// Toggle between 24-hour clock and 12-hour clock (with AM/PM)
    pub fn toggle_time_format(&mut self) {
        self.use_24h_clock = !self.use_24h_clock;
    }

    pub fn select_next_color(&mut self) {
        let current_idx = GOOGLE_EVENT_COLORS
            .iter()
            .position(|c| c.id == self.popup.selected_color_id.as_deref())
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % GOOGLE_EVENT_COLORS.len();
        self.popup.selected_color_id = GOOGLE_EVENT_COLORS[next_idx].id.map(|s| s.to_string());
    }

    pub fn select_prev_color(&mut self) {
        let current_idx = GOOGLE_EVENT_COLORS
            .iter()
            .position(|c| c.id == self.popup.selected_color_id.as_deref())
            .unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            GOOGLE_EVENT_COLORS.len() - 1
        } else {
            current_idx - 1
        };
        self.popup.selected_color_id = GOOGLE_EVENT_COLORS[prev_idx].id.map(|s| s.to_string());
    }

    pub fn cycle_insert_color(&mut self) {
        let current_idx = GOOGLE_EVENT_COLORS
            .iter()
            .position(|c| c.id == self.insert_event.color_id.as_deref())
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % GOOGLE_EVENT_COLORS.len();
        self.insert_event.color_id = GOOGLE_EVENT_COLORS[next_idx].id.map(|s| s.to_string());
    }

    fn move_insert_current_time(&mut self) {
        let now = Local::now();
        self.now = now;
        self.jump_to_time(&now);
        self.insert_event.start_time = now.with_timezone(&Utc);
    }

    /// Insert event details
    fn enter_insert_event_details(&mut self) {
        self.mode = AppMode::InsertEdit;

        let local_start = self.insert_event.start_time.with_timezone(&Local);
        let local_end =
            (self.insert_event.start_time + self.insert_event.duration).with_timezone(&Local);

        let time_fmt = if self.use_24h_clock {
            TIME_FORMAT_24H
        } else {
            TIME_FORMAT_12H
        };
        let start_str = local_start.format(time_fmt).to_string();
        let end_str = local_end.format(time_fmt).to_string();

        self.sel_cal_ind = None;

        // modify the textarea instead of creating new ones to preserve styles
        self.popup.summary.clear();
        self.popup.description.clear();

        self.popup.start_time.clear();
        self.popup.end_time.clear();
        self.popup.start_time.insert_str(start_str);
        self.popup.end_time.insert_str(end_str);
        self.popup.selected_color_id = self.insert_event.color_id.clone();
        self.popup.color_dropdown_open = false;

        self.switch_active_field(Default::default());
    }

    /// Visual event details
    fn enter_visual_event_details(&mut self) {
        self.mode = AppMode::VisualEdit;

        let Some(id) = self.get_selected_event_index() else {
            return;
        };

        let event_node = &self.cal_event_nodes[id];

        let local_start = event_node.start_time.with_timezone(&Local);
        let local_end = event_node.end_time.with_timezone(&Local);

        let time_fmt = if self.use_24h_clock {
            TIME_FORMAT_24H
        } else {
            TIME_FORMAT_12H
        };
        let start_str = local_start.format(time_fmt).to_string();
        let end_str = local_end.format(time_fmt).to_string();

        self.sel_cal_ind = self
            .cal_infos
            .iter()
            .position(|c| c.id == event_node.cal_info.id);

        // modify the textarea instead of creating new ones to preserve styles
        self.popup.summary.clear();
        self.popup.description.clear();
        self.popup.start_time.clear();
        self.popup.end_time.clear();

        let clean_summary = event_node.summary.replace('\r', "").replace('\n', " ");
        self.popup.summary.insert_str(&clean_summary);
        if let Some(description) = &event_node.description {
            self.popup.description.insert_str(description);
        }
        self.popup.start_time.insert_str(start_str);
        self.popup.end_time.insert_str(end_str);
        self.popup.selected_color_id = event_node.color_id.clone();
        self.popup.color_dropdown_open = false;

        self.switch_active_field(Default::default());
    }

    /// Resize viewport
    fn update_viewport_from_height(&mut self, term_height: u16) {
        let vertical_overhead = 5;
        let usable_rows = term_height.saturating_sub(vertical_overhead);
        self.viewport_mins = usable_rows * RESOLUTION_IN_MINS;

        // if the window got taller, the max offset shrinks
        let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    /// Sync viewport
    fn sync_viewport_to_cursor(&mut self) {
        let cursor_start_local = self.insert_event.start_time.with_timezone(&Local);
        let cursor_end_local =
            (self.insert_event.start_time + self.insert_event.duration).with_timezone(&Local);

        let start_date = cursor_start_local.date_naive();
        let end_date = cursor_end_local.date_naive();

        // 1. Horizontal sync
        // shift left if the start date moves out of view
        if start_date < self.start_date {
            self.start_date = start_date;
            self.check_pagination();
        // shift right if the end date pushes past the right edge of view
        } else if end_date >= self.start_date + self.num_days {
            self.start_date = end_date - self.num_days + TimeDelta::days(1);
            self.check_pagination();
        }

        // 2. Vertical sync
        let cursor_start_mins =
            (cursor_start_local.hour() * 60 + cursor_start_local.minute()) as u16;
        let cursor_end_mins = cursor_start_mins + self.insert_event.duration.num_minutes() as u16;
        let viewport_top = self.scroll_offset;
        let viewport_bottom = self.scroll_offset + self.viewport_mins;

        // scroll up if the top edge goes above the screen
        if cursor_start_mins < viewport_top {
            self.scroll_offset = cursor_start_mins;
        // scroll down if the bottom edge goves below the screen
        } else if cursor_end_mins >= viewport_bottom {
            let overflow = cursor_end_mins - viewport_bottom;
            let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
            self.scroll_offset = (self.scroll_offset + overflow).min(max_offset);
        }
    }

    fn sync_viewport_to_selected_event(&mut self) {
        let Some(idx) = self.get_selected_event_index() else {
            return;
        };

        let event_node = &self.cal_event_nodes[idx];
        let start_local = event_node.start_time.with_timezone(&Local);
        let end_local = event_node.end_time.with_timezone(&Local);

        let event_start_date = start_local.date_naive();
        let event_end_date = end_local.date_naive();

        // 1. Horizontal sync
        if event_start_date < self.start_date {
            self.start_date = event_start_date;
            self.check_pagination();
        } else if event_end_date >= self.start_date + self.num_days {
            self.start_date = event_start_date - self.num_days + TimeDelta::days(1);
            self.check_pagination();
        }

        // 3. Vertical sync
        let start_mins = (start_local.hour() * 60 + start_local.minute()) as u16;
        let end_mins = (end_local.hour() * 60 + end_local.minute()) as u16;

        let viewport_top = self.scroll_offset;
        let viewport_bottom = self.scroll_offset + self.viewport_mins;

        if start_mins < viewport_top {
            self.scroll_offset = start_mins;
        } else if end_mins > viewport_bottom {
            let overflow = end_mins.saturating_sub(viewport_bottom);
            let max_offset = MINUTES_IN_DAY.saturating_sub(self.viewport_mins);
            self.scroll_offset = (self.scroll_offset + overflow).min(max_offset);
        }
    }

    fn select_first_visible_current_day_event(&mut self) {
        if self.cal_event_nodes.is_empty() {
            self.sel_event_id = None;
            return;
        };
        let current_day = (self.start_date + START_OFFSET)
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let current_day_local = safe_local_datetime(current_day);

        // convert the current viewport top into UTC timestamp
        let viewport_start_utc =
            (current_day_local + TimeDelta::minutes(self.scroll_offset as i64)).with_timezone(&Utc);

        // Find the first event whose end time extends past the top of viewport
        let first_visible_idx = self
            .cal_event_nodes
            .iter()
            .position(|e| e.end_time > viewport_start_utc)
            .unwrap_or(0);

        self.sel_event_id = self
            .cal_event_nodes
            .get(first_visible_idx)
            .map(|e| e.id.clone());
    }

    fn switch_active_field(&mut self, field: ActiveField) {
        self.popup.active_field = field;

        configure_ta(
            &mut self.popup.summary,
            " Summary ",
            self.popup.active_field == ActiveField::Summary,
        );
        configure_ta(
            &mut self.popup.description,
            " Description ",
            self.popup.active_field == ActiveField::Description,
        );
        configure_ta(
            &mut self.popup.start_time,
            " Start Time ",
            self.popup.active_field == ActiveField::StartTime,
        );
        configure_ta(
            &mut self.popup.end_time,
            " End Time ",
            self.popup.active_field == ActiveField::EndTime,
        );
    }
}

fn configure_ta<'a>(ta: &mut TextArea<'a>, title: &'a str, is_active: bool) {
    let mut block = Block::bordered().title(title);

    if is_active {
        block = block.border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
        ta.set_cursor_line_style(Style::default());
        ta.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
    } else {
        block = block.border_style(Style::default().fg(Color::DarkGray));
        ta.set_cursor_style(Style::default());
    };

    ta.set_block(block);
}

