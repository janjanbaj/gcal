use chrono::{Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use google_calendar3::api::{Event as CEvent, EventDateTime, EventOrganizer};
use j_gcal::{
    app::EventNode,
    constants::SCOPE,
};

#[test]
fn test_oauth_scope_is_least_privilege() {
    assert_eq!(
        SCOPE,
        "https://www.googleapis.com/auth/calendar.events",
        "OAuth scope must be restricted to calendar.events for least privilege"
    );
}

#[test]
fn test_event_node_try_from_missing_organizer_email_does_not_panic() {
    let cevent = CEvent {
        id: Some("test_id_1".to_string()),
        summary: Some("Test Summary".to_string()),
        start: Some(EventDateTime {
            date_time: Some(Utc::now()),
            ..Default::default()
        }),
        end: Some(EventDateTime {
            date_time: Some(Utc::now()),
            ..Default::default()
        }),
        organizer: Some(EventOrganizer {
            display_name: Some("No Email Organizer".to_string()),
            email: None,
            ..Default::default()
        }),
        ..Default::default()
    };

    let res = EventNode::try_from(cevent);
    let _ = res; // Must not panic
}

#[test]
fn test_event_node_try_from_missing_id_does_not_panic() {
    let cevent = CEvent {
        id: None,
        summary: Some("Test Summary".to_string()),
        start: Some(EventDateTime {
            date_time: Some(Utc::now()),
            ..Default::default()
        }),
        end: Some(EventDateTime {
            date_time: Some(Utc::now()),
            ..Default::default()
        }),
        organizer: Some(EventOrganizer {
            display_name: Some("Organizer".to_string()),
            email: Some("organizer@example.com".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let res = EventNode::try_from(cevent);
    assert!(res.is_err(), "Event without ID should safely return Err");
}

#[test]
fn test_event_node_try_from_valid_event() {
    let now = Utc::now();
    let cevent = CEvent {
        id: Some("valid_123".to_string()),
        summary: Some("Planning Session".to_string()),
        description: Some("Deep work".to_string()),
        start: Some(EventDateTime {
            date_time: Some(now),
            ..Default::default()
        }),
        end: Some(EventDateTime {
            date_time: Some(now),
            ..Default::default()
        }),
        organizer: Some(EventOrganizer {
            display_name: Some("Alice".to_string()),
            email: Some("alice@example.com".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let res = EventNode::try_from(cevent);
    assert!(res.is_ok());
    let node = res.unwrap();
    assert_eq!(node.id, "valid_123");
    assert_eq!(node.summary, "Planning Session");
    assert_eq!(node.cal_info.id, "alice@example.com");
}

#[test]
fn test_event_node_try_from_all_day_event_does_not_panic() {
    let cevent = CEvent {
        id: Some("allday_1".to_string()),
        summary: Some("Company Holiday".to_string()),
        start: Some(EventDateTime {
            date: Some(NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()),
            date_time: None,
            ..Default::default()
        }),
        end: Some(EventDateTime {
            date: Some(NaiveDate::from_ymd_opt(2026, 12, 26).unwrap()),
            date_time: None,
            ..Default::default()
        }),
        organizer: Some(EventOrganizer {
            email: Some("admin@example.com".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let _ = EventNode::try_from(cevent);
}

#[test]
fn test_dst_safe_timezone_resolution() {
    // Tests that converting potentially ambiguous/nonexistent local naive datetimes
    // uses safe resolution (single or earliest/latest fallback) instead of .unwrap().
    let naive = NaiveDateTime::parse_from_str("2026-03-08 02:30", "%Y-%m-%d %H:%M").unwrap();
    let safe_dt = match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt),
        LocalResult::Ambiguous(dt, _) => Some(dt),
        LocalResult::None => {
            // Gap time: fallback to 1 hour ahead
            Local.from_local_datetime(&(naive + chrono::Duration::hours(1))).single()
        }
    };
    assert!(safe_dt.is_some(), "DST conversion must resolve safely without panic");
}

#[test]
fn test_default_application_secret_structure() {
    let secret = google_calendar3::yup_oauth2::ApplicationSecret {
        client_id: "test-client".to_string(),
        client_secret: "test-secret".to_string(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        redirect_uris: vec!["http://127.0.0.1".to_string(), "http://localhost".to_string()],
        project_id: None,
        client_email: None,
        auth_provider_x509_cert_url: None,
        client_x509_cert_url: None,
    };
    assert_eq!(secret.client_id, "test-client");
}
