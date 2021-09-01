use anyhow::Result;
use std::path::PathBuf;
use structopt::StructOpt;

mod reload;

#[derive(StructOpt, Debug)]
#[structopt(name = "k3k-controller")]
pub struct Args {
    #[structopt(
        short,
        long,
        default_value = "config.toml",
        help = "Specify path to configuration file"
    )]
    pub config: PathBuf,

    /// Triggers a reload of the Janus Server configuration
    #[structopt(long)]
    pub reload: bool,
}

impl Args {
    /// Returns true if we want to startup the controller after we finished the cli part
    pub fn controller_should_start(&self) -> bool {
        !self.reload
    }
}

/// Parses the CLI-Arguments into [`Args`]
///
/// Also runs (optional) cli commands if necessary
pub fn parse_args() -> Result<Args> {
    let args = Args::from_args();

    if args.reload {
        reload::trigger_reload()?;
    }

    Ok(args)
}
