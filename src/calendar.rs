use chrono::NaiveDate;
use futures::stream::{self, StreamExt, TryStreamExt};
use google_calendar3::{
    CalendarHub,
    api::{CalendarListEntry, Event, EventDateTime},
    hyper_rustls::{HttpsConnector, HttpsConnectorBuilder},
    hyper_util::{
        self,
        client::legacy::{Client, connect::HttpConnector},
    },
    yup_oauth2::{
        self, InstalledFlowAuthenticator, InstalledFlowReturnMethod, read_application_secret,
    },
};

use crate::{Config, Result, app::EventNode, constants::SCOPE, logging};

type Hub = CalendarHub<HttpsConnector<HttpConnector>>;

#[derive(Clone)]
pub struct Calendar {
    pub hub: Hub,
    pub calendar_ids: Vec<String>,
}

impl From<EventNode> for Event {
    fn from(value: EventNode) -> Self {
        let (start, end) = if value.is_all_day {
            let start_d = value.start_time.date_naive();
            let end_d = value.end_time.date_naive();
            (
                EventDateTime {
                    date: Some(start_d),
                    ..Default::default()
                },
                EventDateTime {
                    date: Some(end_d),
                    ..Default::default()
                },
            )
        } else {
            (
                EventDateTime {
                    date_time: Some(value.start_time),
                    ..Default::default()
                },
                EventDateTime {
                    date_time: Some(value.end_time),
                    ..Default::default()
                },
            )
        };

        Event {
            id: Some(value.id),
            summary: Some(value.summary),
            description: value.description,
            start: Some(start),
            end: Some(end),
            color_id: value.color_id,
            ..Default::default()
        }
    }
}

impl Calendar {
    pub async fn new(config: Config) -> Result<Self> {
        let _ = dotenvy::dotenv();
        let app_paths = logging::get_app_path();
        let _ = dotenvy::from_path(app_paths.config_dir.join(".env"));
        let _ = dotenvy::from_path(app_paths.data_dir.join(".env"));

        // 1. Load the client_secret.json or use default fallback
        let calendar_ids = config.calendar_ids;
        let secret = match config.client_file {
            Some(path) if path.exists() && !path.as_os_str().is_empty() => read_application_secret(&path).await?,
            _ => {
                let client_id = std::env::var("GCAL_CLIENT_ID")
                    .map(|s| s.trim_matches('"').trim_matches('\'').trim().to_string())
                    .unwrap_or_else(|_| "883838279262-d27s1a8g1k8945t3v1256338b8h9j87b.apps.googleusercontent.com".to_string());
                let client_secret = std::env::var("GCAL_CLIENT_SECRET")
                    .map(|s| s.trim_matches('"').trim_matches('\'').trim().to_string())
                    .unwrap_or_else(|_| "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t".to_string());
                yup_oauth2::ApplicationSecret {
                    client_id,
                    client_secret,
                    token_uri: "https://oauth2.googleapis.com/token".to_string(),
                    auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
                    redirect_uris: vec!["http://localhost".to_string(), "http://127.0.0.1".to_string()],
                    project_id: None,
                    client_email: None,
                    auth_provider_x509_cert_url: None,
                    client_x509_cert_url: None,
                }
            }
        };

        // 2. Set up the OAuth2 authenticator
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .unwrap()
            .https_only()
            .enable_http2()
            .build();
        let executor = hyper_util::rt::TokioExecutor::new();
        // This will open a browser window the first time you run it to grant permissions.
        // The token is then cached locally in "tokencache.json".
        let app_paths = logging::get_app_path();
        let directory = &app_paths.data_dir;
        std::fs::create_dir_all(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700));
        }
        let cache_path = directory.join("tokencache.json");
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        // If cache file already exists, ensure 0600
        #[cfg(unix)]
        {
            if cache_path.exists() {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o600));
            }
        }

        let auth = InstalledFlowAuthenticator::with_client(
            secret,
            InstalledFlowReturnMethod::HTTPRedirect,
            yup_oauth2::client::CustomHyperClientBuilder::from(
                Client::builder(executor.clone()).build(connector),
            ),
        )
        .persist_tokens_to_disk(cache_path)
        .build()
        .await?;

        let mandatory_scope = &[SCOPE];
        auth.token(mandatory_scope).await?;

        // 3. Initialize the Calendar Hub client
        let client = Client::builder(executor).build(
            HttpsConnectorBuilder::new()
                .with_native_roots()
                .unwrap()
                .https_only()
                .enable_http2()
                .build(),
        );
        Ok(Calendar {
            hub: CalendarHub::new(client, auth),
            calendar_ids,
        })
    }

    pub async fn get_calendars_info(&self) -> Result<Vec<CalendarListEntry>> {
        let (_, calendar_list) = self.hub
            .calendar_list()
            .list()
            .add_scope(SCOPE)
            .doit()
            .await?;

        let items = calendar_list.items.unwrap_or_default();
        if items.is_empty() {
            Ok(vec![CalendarListEntry {
                id: Some("primary".to_string()),
                summary: Some("Primary".to_string()),
                background_color: Some("#4285F4".to_string()),
                foreground_color: Some("#ffffff".to_string()),
                ..Default::default()
            }])
        } else {
            Ok(items)
        }
    }

    pub async fn get_events(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<(String, Event)>> {
        // 4. Calculate the start and end of the current week (Monday to Sunday)
        let start_time = crate::app::safe_local_datetime(
            start_date.and_hms_opt(0, 0, 0).unwrap_or_default()
        );
        let end_time = crate::app::safe_local_datetime(
            end_date.and_hms_opt(0, 0, 0).unwrap_or_default()
        );

        let cal_ids = self.calendar_ids.clone();
        stream::iter(cal_ids.into_iter().map(|cal_id| {
            let hub_clone = self.hub.clone();
            async move {
                let (_, event_list) = hub_clone
                    .events()
                    .list(&cal_id)
                    .time_min(start_time.to_utc())
                    .time_max(end_time.to_utc())
                    .single_events(true) // Crucial: expands recurring events into individual instances
                    .order_by("startTime") // Returns them chronologically
                    .add_scope(SCOPE)
                    .doit()
                    .await?;
                let items: Vec<(String, Event)> = event_list
                    .items
                    .unwrap_or_default()
                    .into_iter()
                    .map(|ev| (cal_id.clone(), ev))
                    .collect();
                Ok(items)
            }
        }))
        .buffer_unordered(5)
        .try_concat()
        .await
    }

    pub async fn create_event(&self, mut event_node: EventNode) -> Result<Event> {
        let calendar_id = std::mem::take(&mut event_node.cal_info.id);
        let new_event = event_node.into();

        let (_, created_event) = self
            .hub
            .events()
            .insert(new_event, &calendar_id)
            .add_scope(SCOPE)
            .doit()
            .await?;

        Ok(created_event)
    }

    pub async fn patch_event(&self, mut event_node: EventNode) -> Result<Event> {
        let event_id = event_node.id.clone();
        let calendar_id = std::mem::take(&mut event_node.cal_info.id);
        let patch_event = event_node.into();

        let (_, updated_event) = self
            .hub
            .events()
            .patch(patch_event, &calendar_id, &event_id)
            .add_scope(SCOPE)
            .doit()
            .await?;

        Ok(updated_event)
    }

    pub async fn delete_event(&self, mut event_node: EventNode) -> Result<()> {
        let calendar_id = std::mem::take(&mut event_node.cal_info.id);
        let event_id = event_node.id;

        self.hub
            .events()
            .delete(&calendar_id, &event_id)
            .add_scope(SCOPE)
            .doit()
            .await?;

        Ok(())
    }
}
