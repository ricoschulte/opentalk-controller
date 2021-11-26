use anyhow::Result;
use std::path::PathBuf;
use structopt::StructOpt;

mod db_schema;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "xtask",
    about = "This binary defines auxiliary ad-hoc scripts."
)]
enum XTasks {
    /// Create the diesel DB schema file
    GenerateDbSchema {
        #[structopt(long, env = "POSTGRES_URL")]
        postgres_url: Option<url::Url>,
        #[structopt(long, env = "DATABASE_NAME")]
        database_name: Option<String>,
    },
    /// Runs the db-storage crates migrations and verifies if the present schema.rs is correct.
    VerifyDbSchema {
        #[structopt(long, env = "POSTGRES_URL")]
        postgres_url: Option<url::Url>,
        #[structopt(long, env = "DATABASE_NAME")]
        database_name: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut builder = env_logger::Builder::new();
    builder
        .filter_level(log::LevelFilter::Info)
        .format_timestamp(None)
        .parse_default_env();
    builder.init();

    let opt = XTasks::from_args();
    match opt {
        XTasks::GenerateDbSchema {
            postgres_url,
            database_name,
        } => db_schema::generate_db_schema(postgres_url, database_name).await?,
        XTasks::VerifyDbSchema {
            postgres_url,
            database_name,
        } => db_schema::verify_db_schema(postgres_url, database_name).await?,
    };

    Ok(())
}

/// Searches for a project root dir, which is a directory that contains a
/// `Cargo.toml` file that defines the project's [cargo workspace][cargo-workspace]).
///
/// It uses the value of [`cargo metadata`][cargo-metadata] `workspace_root`.
///
/// [cargo-metadata]: https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
/// [cargo-workspace]: https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html
pub fn locate_project_root() -> Result<PathBuf> {
    let cmd = cargo_metadata::MetadataCommand::new();

    let metadata = cmd.exec().unwrap();
    let workspace_root = metadata.workspace_root;

    Ok(workspace_root.into())
}
