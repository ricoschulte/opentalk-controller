use anyhow::{Context, Result};

mod db;
mod settings;

#[tokio::main]
async fn main() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Warn)
        .chain(std::io::stdout())
        .apply()
        .context("Error while setting up logger")?;

    let settings = settings::Settings::new("config.toml")?;

    db::migrations::start_migration(&settings.database).await?;

    Ok(())
}
