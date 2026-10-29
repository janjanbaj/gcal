use crate::{App, Result, logging::initialize_logging};

pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();
    let app_paths = crate::logging::get_app_path();
    let _ = dotenvy::from_path(app_paths.config_dir.join(".env"));
    let _ = dotenvy::from_path(app_paths.data_dir.join(".env"));

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        default_hook(info);
    }));

    initialize_logging()?;

    let app = App::new().await?;

    let terminal = ratatui::init();
    let result = app.run(terminal).await;
    ratatui::restore();

    result
}
