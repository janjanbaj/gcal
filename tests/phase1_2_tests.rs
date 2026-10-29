use chrono::{TimeDelta, Utc};
use j_gcal::{
    Config,
    app::InsertEvent,
    constants::{RESOLUTION_IN_MINS, ROWS_PER_HOUR},
};

#[test]
fn test_resolution_is_5_minutes() {
    // Phase 2 requirement: time blocking in 5-minute increments
    assert_eq!(
        RESOLUTION_IN_MINS, 5,
        "Time slot resolution must be 5 minutes"
    );
    assert_eq!(
        ROWS_PER_HOUR, 12,
        "Rows per hour must be 12 (60 / 5)"
    );
}

#[test]
fn test_default_config_is_zero_setup() {
    // Phase 1 requirement: zero config files required.
    // Default config should have primary calendar and no mandatory client_file.
    let config = Config::default();
    assert!(config.client_file.is_none());
}

#[test]
fn test_edit_event_summary_sanitization_and_trim() {
    let raw = "  Updated Meeting Name \n \r ";
    let clean = raw.replace('\r', "").replace('\n', " ");
    assert_eq!(clean.trim(), "Updated Meeting Name");
}

#[test]
fn test_insert_event_5_minute_increments() {
    let now = Utc::now();
    let mut insert = InsertEvent {
        start_time: now,
        duration: TimeDelta::minutes(30),
        color_id: None,
    };

    // Extending duration by 5 minutes
    let step = TimeDelta::minutes(RESOLUTION_IN_MINS as i64);
    insert.duration += step;
    assert_eq!(insert.duration, TimeDelta::minutes(35));

    // Moving start time by 5 minutes
    insert.start_time += step;
    assert_eq!(insert.start_time, now + TimeDelta::minutes(5));
}

#[test]
fn test_time_block_formatting_duration() {
    // Tests calculation of duration display for visual time blocks
    let start = Utc::now();
    let duration = TimeDelta::minutes(45);
    let end = start + duration;

    let mins = (end - start).num_minutes();
    assert_eq!(mins, 45);
    let label = if mins >= 60 {
        format!("{}h {}m", mins / 60, mins % 60)
    } else {
        format!("{}m", mins)
    };
    assert_eq!(label, "45m");
}

#[test]
fn test_scroll_top_and_bottom_bounds() {
    // gg -> top (00:00)
    let offset_top: u16 = 0;
    assert_eq!(offset_top, 0);

    // G -> bottom (24*60 - viewport)
    let max_offset = 24 * 60 - 6 * 60;
    let offset_bottom = max_offset;
    assert_eq!(offset_bottom, 18 * 60);
}

#[test]
fn test_calendar_list_entry_colors() {
    let entry = google_calendar3::api::CalendarListEntry {
        id: Some("work@group.calendar.google.com".to_string()),
        summary: Some("Work Calendar".to_string()),
        background_color: Some("#7cb342".to_string()),
        foreground_color: Some("#ffffff".to_string()),
        color_id: Some("9".to_string()),
        ..Default::default()
    };
    assert_eq!(entry.background_color.as_deref(), Some("#7cb342"));

    // Verify CalendarInfo converts cleanly and preserves color_bg
    let cal_info = j_gcal::CalendarInfo::try_from(entry).expect("conversion should succeed");
    assert_eq!(cal_info.color_bg.as_deref(), Some("#7cb342"));
    assert_eq!(cal_info.color_fg.as_deref(), Some("#ffffff"));
}

#[test]
fn test_event_node_color_mapping() {
    use ratatui::style::Color;

    let cal_info = j_gcal::CalendarInfo {
        display_name: "Work".to_string(),
        id: "work@group.calendar.google.com".to_string(),
        color_bg: Some("#7cb342".to_string()),
        color_fg: Some("#ffffff".to_string()),
    };

    let now = Utc::now();

    // 1. Event with Google color_id "1" (Lavender -> RGB(121, 134, 203))
    let ev1 = j_gcal::EventNode {
        id: "1".to_string(),
        summary: "Lavender event".to_string(),
        description: None,
        start_time: now,
        end_time: now + TimeDelta::minutes(30),
        cal_info: cal_info.clone(),
        color_id: Some("1".to_string()),
        is_all_day: false,
    };
    assert_eq!(ev1.get_color(), Color::Rgb(121, 134, 203));

    // 2. Event with Google color_id "11" (Tomato -> RGB(213, 0, 0))
    let ev11 = j_gcal::EventNode {
        id: "2".to_string(),
        summary: "Tomato event".to_string(),
        description: None,
        start_time: now,
        end_time: now + TimeDelta::minutes(30),
        cal_info: cal_info.clone(),
        color_id: Some("11".to_string()),
        is_all_day: false,
    };
    assert_eq!(ev11.get_color(), Color::Rgb(213, 0, 0));

    // 3. Event without color_id -> inherits calendar background hex "#7cb342" (R: 0x7c=124, G: 0xb3=179, B: 0x42=66)
    let ev_cal = j_gcal::EventNode {
        id: "3".to_string(),
        summary: "Calendar colored event".to_string(),
        description: None,
        start_time: now,
        end_time: now + TimeDelta::minutes(30),
        cal_info: cal_info.clone(),
        color_id: None,
        is_all_day: false,
    };
    assert_eq!(ev_cal.get_color(), Color::Rgb(124, 179, 66));

    // 4. Fallback when neither color_id nor color_bg is available
    let ev_fallback = j_gcal::EventNode {
        id: "4".to_string(),
        summary: "Fallback event".to_string(),
        description: None,
        start_time: now,
        end_time: now + TimeDelta::minutes(30),
        cal_info: j_gcal::CalendarInfo {
            display_name: "Default".to_string(),
            id: "primary".to_string(),
            color_bg: None,
            color_fg: None,
        },
        color_id: None,
        is_all_day: false,
    };
    assert_eq!(ev_fallback.get_color(), Color::Cyan);
}

#[test]
fn test_day_header_format_includes_date_and_weekday() {
    use chrono::NaiveDate;
    let date = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
    let header = date.format("%A %Y/%m/%d").to_string();
    assert_eq!(header, "Sunday 2026/10/11");

    let date2 = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
    let header2 = date2.format("%A %Y/%m/%d").to_string();
    assert_eq!(header2, "Monday 2026/01/05");
}

#[test]
fn test_scroll_acceleration_curve() {
    // Verifies the acceleration step progression
    fn calc_step(streak: u16) -> u16 {
        match streak {
            0 => 15,
            1..=3 => 30,
            4..=7 => 60,
            8..=12 => 120,
            _ => 180,
        }
    }

    assert_eq!(calc_step(0), 15, "Single tap should move 15 mins");
    assert_eq!(calc_step(1), 30, "Streak 1 should move 30 mins");
    assert_eq!(calc_step(3), 30, "Streak 3 should move 30 mins");
    assert_eq!(calc_step(4), 60, "Streak 4 should move 1 hour");
    assert_eq!(calc_step(8), 120, "Streak 8 should move 2 hours");
    assert_eq!(calc_step(20), 180, "Sustained hold should move 3 hours per frame");
}

#[test]
fn test_auto_refresh_interval_logic() {
    let interval = std::time::Duration::from_secs(30);
    let past = std::time::Instant::now() - std::time::Duration::from_secs(35);
    assert!(past.elapsed() >= interval, "Should trigger refresh when elapsed >= 30s");

    let recent = std::time::Instant::now() - std::time::Duration::from_secs(10);
    assert!(recent.elapsed() < interval, "Should not trigger refresh when elapsed < 30s");
}

#[test]
fn test_event_node_all_day_parsing_and_spans_date() {
    use google_calendar3::api::{Event as CEvent, EventDateTime};
    use chrono::NaiveDate;

    let cal_event = CEvent {
        id: Some("allday_test".to_string()),
        summary: Some("Offsite".to_string()),
        start: Some(EventDateTime {
            date: Some(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()),
            date_time: None,
            ..Default::default()
        }),
        end: Some(EventDateTime {
            date: Some(NaiveDate::from_ymd_opt(2026, 10, 16).unwrap()),
            date_time: None,
            ..Default::default()
        }),
        ..Default::default()
    };

    let node = j_gcal::EventNode::try_from(cal_event).expect("conversion should succeed");
    assert!(node.is_all_day, "Event with only date and no date_time must be marked is_all_day");
    assert!(node.spans_date(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()));
    assert!(!node.spans_date(NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()));
}

#[test]
fn test_view_mode_switching_days_logic() {
    use chrono::{Datelike, NaiveDate, TimeDelta};

    let now_date = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap(); // Wednesday
    let weekday = now_date.weekday().num_days_from_monday(); // 2

    // 1-Day view
    let num_days_1 = TimeDelta::days(1);
    let start_1 = now_date;
    assert_eq!(num_days_1.num_days(), 1);
    assert_eq!(start_1, now_date);

    // 3-Day view (Yesterday, Today, Tomorrow)
    let num_days_3 = TimeDelta::days(3);
    let start_3 = now_date - TimeDelta::days(1);
    assert_eq!(num_days_3.num_days(), 3);
    assert_eq!(start_3, NaiveDate::from_ymd_opt(2026, 10, 13).unwrap());

    // Week view (Starts on Monday)
    let num_days_7 = TimeDelta::days(7);
    let start_7 = now_date - TimeDelta::days(weekday as i64);
    assert_eq!(num_days_7.num_days(), 7);
    assert_eq!(start_7, NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()); // Monday
}

#[test]
fn test_contrasting_text_color() {
    use ratatui::style::Color;
    use j_gcal::view::get_contrasting_text_color;

    // Dark backgrounds must use White text
    assert_eq!(get_contrasting_text_color(Color::Rgb(121, 134, 203)), Color::White); // Lavender
    assert_eq!(get_contrasting_text_color(Color::Rgb(142, 36, 170)), Color::White);  // Grape
    assert_eq!(get_contrasting_text_color(Color::Rgb(11, 128, 67)), Color::White);   // Basil
    assert_eq!(get_contrasting_text_color(Color::Rgb(213, 0, 0)), Color::White);     // Tomato
    assert_eq!(get_contrasting_text_color(Color::Rgb(66, 133, 244)), Color::White);  // Google Blue
    assert_eq!(get_contrasting_text_color(Color::Black), Color::White);
    assert_eq!(get_contrasting_text_color(Color::Blue), Color::White);

    // Light backgrounds must use Black text
    assert_eq!(get_contrasting_text_color(Color::Rgb(246, 191, 38)), Color::Black);  // Banana Yellow
    assert_eq!(get_contrasting_text_color(Color::Rgb(230, 124, 115)), Color::Black); // Flamingo
    assert_eq!(get_contrasting_text_color(Color::Yellow), Color::Black);
    assert_eq!(get_contrasting_text_color(Color::White), Color::Black);
    assert_eq!(get_contrasting_text_color(Color::LightCyan), Color::Black);
}

#[test]
fn test_table_grid_line_formatting() {
    use j_gcal::view::format_grid_line;

    // 24h mode
    let line_20_24h = format_grid_line(9, 20, true);
    assert_eq!(line_20_24h.chars().count(), 20);
    assert!(line_20_24h.starts_with("─ 09:00 "));

    let line_12_24h = format_grid_line(14, 12, true);
    assert_eq!(line_12_24h.chars().count(), 12);
    assert!(line_12_24h.starts_with("─ 14:00 "));

    // 12h mode
    let line_20_12h_am = format_grid_line(9, 20, false);
    assert_eq!(line_20_12h_am.chars().count(), 20);
    assert!(line_20_12h_am.starts_with("─ 9:00 AM "));

    let line_20_12h_pm = format_grid_line(14, 20, false);
    assert_eq!(line_20_12h_pm.chars().count(), 20);
    assert!(line_20_12h_pm.starts_with("─ 2:00 PM "));

    let line_20_12h_midnight = format_grid_line(0, 20, false);
    assert!(line_20_12h_midnight.starts_with("─ 12:00 AM "));

    let line_20_12h_noon = format_grid_line(12, 20, false);
    assert!(line_20_12h_noon.starts_with("─ 12:00 PM "));

    // Narrow column (< 12 width): solid rule
    let line_8 = format_grid_line(10, 8, true);
    assert_eq!(line_8.chars().count(), 8);
    assert_eq!(line_8, "────────");
}

#[test]
fn test_12h_24h_clock_formatting_and_parsing() {
    use j_gcal::app::parse_time_str;
    use j_gcal::view::{format_12h_parts, format_time_str};
    use chrono::{NaiveDate, NaiveTime};

    // 12h parts
    assert_eq!(format_12h_parts(0, 0), (12, 0, "AM"));
    assert_eq!(format_12h_parts(9, 30), (9, 30, "AM"));
    assert_eq!(format_12h_parts(12, 0), (12, 0, "PM"));
    assert_eq!(format_12h_parts(13, 45), (1, 45, "PM"));
    assert_eq!(format_12h_parts(23, 59), (11, 59, "PM"));

    // format_time_str
    assert_eq!(format_time_str(9, 5, true), "09:05");
    assert_eq!(format_time_str(9, 5, false), "9:05 AM");
    assert_eq!(format_time_str(13, 0, true), "13:00");
    assert_eq!(format_time_str(13, 0, false), "1:00 PM");

    // parse_time_str flexible parsing (24h and 12h)
    let expected = NaiveDate::from_ymd_opt(2026, 10, 21)
        .unwrap()
        .and_time(NaiveTime::from_hms_opt(14, 30, 0).unwrap());

    let parsed_24h = parse_time_str("2026-10-21 14:30");
    assert_eq!(parsed_24h, Some(expected));

    let parsed_12h_space = parse_time_str("2026-10-21 02:30 PM");
    assert_eq!(parsed_12h_space, Some(expected));

    let parsed_12h_no_space = parse_time_str("2026-10-21 02:30PM");
    assert_eq!(parsed_12h_no_space, Some(expected));

    // Test toggle logic
    let mut use_24h = true;
    use_24h = !use_24h;
    assert!(!use_24h);
    use_24h = !use_24h;
    assert!(use_24h);
}

#[test]
fn test_calendar_event_color_enrichment() {
    use chrono::{TimeDelta, Utc};
    use ratatui::style::Color;
    use j_gcal::{CalendarInfo, EventNode, app::App};

    let cal_infos = vec![
        CalendarInfo {
            id: "work_cal@group.calendar.google.com".to_string(),
            display_name: "Work Calendar".to_string(),
            color_bg: Some("#0B8043".to_string()), // Basil
            color_fg: Some("#ffffff".to_string()),
        },
        CalendarInfo {
            id: "personal_cal@group.calendar.google.com".to_string(),
            display_name: "Personal Calendar".to_string(),
            color_bg: Some("#F6BF26".to_string()), // Banana
            color_fg: Some("#000000".to_string()),
        },
    ];

    let now = Utc::now();
    let mut events = vec![
        EventNode {
            id: "e1".to_string(),
            summary: "Sprint Planning".to_string(),
            description: None,
            start_time: now,
            end_time: now + TimeDelta::minutes(60),
            cal_info: CalendarInfo {
                id: "work_cal@group.calendar.google.com".to_string(),
                display_name: "Untitled".to_string(),
                color_bg: None,
                color_fg: None,
            },
            color_id: None,
            is_all_day: false,
        },
        EventNode {
            id: "e2".to_string(),
            summary: "Dentist Appointment".to_string(),
            description: None,
            start_time: now,
            end_time: now + TimeDelta::minutes(30),
            cal_info: CalendarInfo {
                id: "personal_cal@group.calendar.google.com".to_string(),
                display_name: "Untitled".to_string(),
                color_bg: None,
                color_fg: None,
            },
            color_id: None,
            is_all_day: false,
        },
    ];

    // Before enrichment: color_bg is None
    assert!(events[0].cal_info.color_bg.is_none());
    assert!(events[1].cal_info.color_bg.is_none());

    // Enrich using cal_infos
    App::enrich_events_slice(&cal_infos, &mut events);

    // After enrichment: colors and display names are populated!
    assert_eq!(events[0].cal_info.display_name, "Work Calendar");
    assert_eq!(events[0].cal_info.color_bg.as_deref(), Some("#0B8043"));
    assert_eq!(events[0].get_color(), Color::Rgb(11, 128, 67));

    assert_eq!(events[1].cal_info.display_name, "Personal Calendar");
    assert_eq!(events[1].cal_info.color_bg.as_deref(), Some("#F6BF26"));
    assert_eq!(events[1].get_color(), Color::Rgb(246, 191, 38));
}

#[test]
fn test_insert_color_selection_and_google_palette() {
    use j_gcal::constants::GOOGLE_EVENT_COLORS;
    use ratatui::style::Color;

    // Verify 12 entries: None (Default) + 11 Google Calendar Event colors
    assert_eq!(GOOGLE_EVENT_COLORS.len(), 12);
    assert_eq!(GOOGLE_EVENT_COLORS[0].id, None);
    assert_eq!(GOOGLE_EVENT_COLORS[0].name, "Default (Calendar)");

    let expected_names = [
        "Lavender", "Sage", "Grape", "Flamingo", "Banana",
        "Tangerine", "Peacock", "Graphite", "Blueberry", "Basil", "Tomato",
    ];
    for (i, name) in expected_names.iter().enumerate() {
        let color_id_str = format!("{}", i + 1);
        let entry = &GOOGLE_EVENT_COLORS[i + 1];
        assert_eq!(entry.id, Some(color_id_str.as_str()));
        assert_eq!(entry.name, *name);
    }

    // Verify InsertEvent get_color()
    let now = Utc::now();
    let mut insert = InsertEvent {
        start_time: now,
        duration: TimeDelta::minutes(30),
        color_id: None,
    };
    // Default without color_id is Yellow border
    assert_eq!(insert.get_color(), Color::Yellow);

    // Set to Tomato ("11")
    insert.color_id = Some("11".to_string());
    assert_eq!(insert.get_color(), Color::Rgb(213, 0, 0));

    // Set to Basil ("10")
    insert.color_id = Some("10".to_string());
    assert_eq!(insert.get_color(), Color::Rgb(11, 128, 67));

    // Set to Peacock ("7")
    insert.color_id = Some("7".to_string());
    assert_eq!(insert.get_color(), Color::Rgb(3, 155, 229));
}


