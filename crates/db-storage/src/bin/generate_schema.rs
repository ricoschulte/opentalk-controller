//! Helper binary that creates the schema.rs file
use anyhow::{Context, Result};
use database::query_helper;
use diesel::{Connection, PgConnection, RunQueryDsl};
use k3k_db_storage::migrations::migrate_from_url;
use rand::Rng;
use std::env::current_dir;
use std::io::Write;
use std::process::{Command, Stdio};

#[tokio::main]
pub async fn main() -> Result<()> {
    let mut builder = env_logger::Builder::new();
    builder
        .filter_level(log::LevelFilter::Info)
        .format_timestamp(None)
        .parse_default_env();
    builder.init();

    log::info!("Connecting to database");
    let base_url = std::env::var("POSTGRES_BASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:password123@localhost:5432".to_owned());

    let random: u8 = rand::thread_rng().gen();
    let (db_name, drop_db) = std::env::var("DATABASE_NAME")
        .map(|var| (var, false))
        .unwrap_or_else(|_| (format!("k3k_migration_{}", random), true));
    let postgres_url = format!("{}/{}", base_url, db_name);

    if PgConnection::establish(&postgres_url).is_err() {
        let (database, postgres_url) = change_database_of_url(&postgres_url, "postgres");
        log::info!("Creating database: {}", database);
        let mut conn = PgConnection::establish(&postgres_url)?;
        query_helper::create_database(&database).execute(&mut conn)?;
    }

    log::info!("Applying migrations to database:");
    migrate_from_url(&postgres_url)
        .await
        .expect("Unable to migrate database");

    log::info!("Generate schema.rs:");
    let diesel_output = run_diesel_cli(&postgres_url);
    match diesel_output {
        Ok(output) => {
            let rustfmt_output = run_rustfmt(&output);
            match rustfmt_output {
                Ok(output) => {
                    let target_file = current_dir()?.join("src").join("schema.rs");
                    let file_exist = match std::fs::metadata(&target_file) {
                        Ok(_) => Ok(true),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                        Err(error) => Err(error),
                    }?;
                    if file_exist {
                        log::info!("{} exists. Deleting it first.", target_file.display())
                    }
                    std::fs::write(target_file, &output)?;
                }
                Err(e) => log::error!("{}", e),
            }
        }
        Err(e) => log::error!("{}", e),
    }

    if drop_db {
        log::info!("Dropping temporary database");

        let (database, postgres_url) = change_database_of_url(&postgres_url, "postgres");
        log::info!("Creating database: {}", database);
        let mut conn = PgConnection::establish(&postgres_url)?;
        query_helper::drop_database(&database).execute(&mut conn)?;
    } else {
        log::warn!("Did not drop database, as it was specified in an env var");
    }
    Ok(())
}

fn change_database_of_url(database_url: &str, default_database: &str) -> (String, String) {
    let base = ::url::Url::parse(database_url).unwrap();
    let database = base.path_segments().unwrap().last().unwrap().to_owned();
    let mut new_url = base.join(default_database).unwrap();
    new_url.set_query(base.query());
    (database, new_url.into())
}

fn run_diesel_cli(db_url: &str) -> Result<String> {
    let output = Command::new("diesel")
        .arg("--database-url")
        .arg(&db_url)
        .arg("print-schema")
        .output()?;

    if !output.status.success() {
        anyhow::bail!("diesel-cli executed with failing error code");
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn run_rustfmt(stdin: &str) -> Result<String> {
    let mut child = Command::new("rustfmt")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .as_mut()
        .with_context(|| "Child process stdin has not been captured!")?
        .write_all(stdin.as_bytes())?;

    let output = child.wait_with_output()?;

    if !output.status.success() {
        anyhow::bail!("Command executed with failing error code");
    }
    Ok(String::from_utf8(output.stdout)?)
}
