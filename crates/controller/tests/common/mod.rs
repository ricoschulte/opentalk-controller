use anyhow::{bail, Context, Result};
use k3k_controller_client::{Config, K3KSession};
use openidconnect::url::Url;
use regex::Regex;
use std::process::Stdio;
use std::rc::Rc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{self, Child};

/// Setup the API client
///
/// Runs the OIDC sso routine and returns a new K3KSession that contains all necessary tokens.
///
/// Creates a new client config from the environment variables
///
/// # Example config:
/// ```sh
/// K3K_CLIENT__K3K_URL=http://localhost:8000
/// K3K_CLIENT__CLIENT_ID=Frontend
/// K3K_CLIENT__REDIRECT_URL=http://localhost:8081/auth/keycloak/sso
/// ```
///
/// # Note
/// When a environment variable is not set, it defaults to their respective value seen in the
/// example above.
pub async fn setup_client(user: &str, password: &str) -> Result<K3KSession> {
    let k3k_url = Url::parse(
        std::env::var("K3K_CLIENT__K3K_URL")
            .unwrap_or("http://localhost:11311".to_string())
            .as_str(),
    )
    .context("Unable to parse k3k url")?;

    let client_id = std::env::var("K3K_CLIENT__CLIENT_ID").unwrap_or("Frontend".to_string());

    let redirect_url = Url::parse(
        std::env::var("K3K_CLIENT__REDIRECT_URL")
            .unwrap_or("http://localhost:8081/auth/keycloak/sso".to_string())
            .as_str(),
    )
    .context("Unable to parse redirect url")?;

    let conf = Config {
        k3k_url,
        client_id,
        redirect_url,
    };

    let oidc = conf.openid_connect_discover().await?;

    let auth_tokens = oidc.authenticate(user, password).await?;

    let session = K3KSession::new(Rc::new(conf), auth_tokens);

    Ok(session)
}

/// Starts a controller instance as a child process
///
/// The controller will be stopped when the process handle is dropped or its `kill` method is called.
/// Returns the the process handle of the controller.
pub async fn run_controller() -> Result<Child> {
    let postgres_base_url = std::env::var("POSTGRES_BASE_URL")
        .unwrap_or("postgres://postgres:password123@localhost:5432".to_string());

    let database_name = std::env::var("DATABASE_NAME").unwrap_or("k3k_test".to_string());

    let mut controller_proc = process::Command::new("../../target/debug/k3k-controller")
        .env(
            "K3K_CTRL_DATABASE__URL",
            format!("{}/{}", postgres_base_url, database_name),
        )
        .args(&["-c", "tests/test-config.toml"])
        .kill_on_drop(true)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to start controller")?;

    let ctl_out = controller_proc
        .stdout
        .take()
        .context("can not acquire controller output")?;

    let mut reader = BufReader::new(ctl_out).lines();
    let ctl_start_pattern = Regex::new(r#".*Startup finished"#).unwrap(); //panic on bad regex pattern only

    while let Some(ref line) = reader.next_line().await? {
        log::info!(target:"controller_log", "{}", line);
        if ctl_start_pattern.is_match(line) {
            break;
        }
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
