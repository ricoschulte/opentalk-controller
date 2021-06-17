use anyhow::{bail, Context, Result};
use k3k_client::k3k;
use k3k_client::k3k::K3KSession;
use regex::Regex;
use std::process::Stdio;
use std::rc::Rc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{self, Child};
use tokio_postgres::NoTls;

pub async fn setup_client(user: &str, password: &str) -> Result<K3KSession> {
    // default client config TODO: set via environment
    let conf = k3k::default_config();

    let oidc = conf.openid_discover().await?;

    let auth_tokens = oidc.authenticate(user, password).await?;

    let session = k3k::K3KSession::new(Rc::new(conf), auth_tokens);

    Ok(session)
}

pub async fn run_controller() -> Result<Child> {
    let mut controller_proc = process::Command::new("./target/debug/k3k-controller")
        //.current_dir("../")
        .args(&["-v", "-c", "tests/test-config.toml"])
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to start controller")?;

    let ctl_out = controller_proc
        .stdout
        .take()
        .context("can not acquire controller output")?;

    let mut reader = BufReader::new(ctl_out).lines();
    let ctl_start_pattern = Regex::new(r#".*Startup finished$"#).unwrap(); //panic on bad regex pattern only

    while let Some(ref line) = reader.next_line().await? {
        log::info!(target:"controller_log", "{}", line);
        if ctl_start_pattern.is_match(line) {
            break;
        }

        //todo: stuck here if controller errors?
    }

    let _log_task = tokio::spawn(async move {
        while let Some(line) = reader.next_line().await? {
            log::info!(target:"controller_log", "{}", line);
        }
        Ok::<(), std::io::Error>(())
    });

    if let Some(_exit_code) = controller_proc.try_wait()? {
        bail!("Controller process died");
    }
    Ok(controller_proc)
}

/// Cleanup the
pub async fn cleanup_database() -> Result<()> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or("postgres://postgres:password123@localhost:5432/k3k".to_string());

    let (mut client, connection) = tokio_postgres::connect(&url, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            log::error!("connection error: {}", e);
        }
    });

    let transaction = client.transaction().await?;

    let tables = transaction
        .query(
            r#"SELECT tablename FROM pg_tables WHERE schemaname='public';"#,
            &[],
        )
        .await
        .context("unable to select tables from database")?;

    let tables = tables
        .iter()
        .map(|row| row.get::<_, &str>("tablename"))
        .collect::<Vec<_>>();

    if tables.is_empty() {
        log::debug!("No tables to drop");
        return Ok(());
    }

    let drop_tables_query = format!("DROP TABLE IF EXISTS {} CASCADE;", tables.join(", "));

    transaction
        .execute(drop_tables_query.as_str(), &[])
        .await
        .with_context(|| format!("unable to drop tables with query: '{}'", drop_tables_query))?;

    transaction
        .commit()
        .await
        .context("unable to commit transaction")?;

    Ok(())
}

pub fn setup_logging() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()
        .context("Failed to setup logging utility")
}
