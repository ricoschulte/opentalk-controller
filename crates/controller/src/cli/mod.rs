use anyhow::{Context, Result};
use controller_shared::settings::Settings;
use structopt::StructOpt;

mod acl;
mod fix_acl;
mod reload;

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "k3k-controller")]
pub struct Args {
    #[structopt(
        short,
        long,
        default_value = "config.toml",
        help = "Specify path to configuration file"
    )]
    pub config: String,

    /// Triggers a reload of the Janus Server configuration
    #[structopt(long)]
    pub reload: bool,

    #[structopt(subcommand)]
    cmd: Option<SubCommand>,

    #[structopt(long)]
    version: bool,
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(rename_all = "kebab_case")]
enum SubCommand {
    /// Rebuild ACLs based on current data
    FixAcl {
        /// Do not add user roles
        #[structopt(long = "no-user-roles", parse(from_flag = std::ops::Not::not))]
        user_roles: bool,
        /// Do not add user groups
        #[structopt(long = "no-user-groups", parse(from_flag = std::ops::Not::not))]
        user_groups: bool,
        /// Do not add room owner read/write access
        #[structopt(long = "no-room-creators", parse(from_flag = std::ops::Not::not))]
        room_creators: bool,
    },
    /// Modify the ACLs.
    Acl(AclSubCommand),
    /// Migrate the db. This is done automatically during start of the controller,
    /// but can be done without starting the controller using this command.
    MigrateDb,
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(rename_all = "kebab_case")]
pub(crate) enum AclSubCommand {
    /// Allows all users access to all rooms
    UsersHaveAccessToAllRooms {
        /// Enable/Disable
        #[structopt(subcommand)]
        action: EnableDisable,
    },
}

#[derive(StructOpt, Debug, Clone)]
#[structopt(rename_all = "kebab_case")]
pub(crate) enum EnableDisable {
    /// enable
    Enable,
    /// disable
    Disable,
}

impl Args {
    /// Returns true if we want to startup the controller after we finished the cli part
    pub fn controller_should_start(&self) -> bool {
        !(self.reload || self.cmd.is_some() || self.version)
    }
}

/// Parses the CLI-Arguments into [`Args`]
///
/// Also runs (optional) cli commands if necessary
pub async fn parse_args() -> Result<Args> {
    let args = Args::from_args();

    if args.version {
        print_version()
    }

    if args.reload {
        reload::trigger_reload()?;
    }
    if let Some(sub_command) = args.cmd.clone() {
        let settings = Settings::load(&args.config)?;
        match sub_command {
            SubCommand::FixAcl {
                user_roles,
                user_groups,
                room_creators,
            } => {
                let config = fix_acl::FixAclConfig {
                    user_roles,
                    user_groups,
                    room_creators,
                };
                fix_acl::fix_acl(settings, config).await?;
            }
            SubCommand::Acl(subcommand) => {
                acl::acl(settings, subcommand).await?;
            }
            SubCommand::MigrateDb => {
                let result = db_storage::migrations::migrate_from_url(&settings.database.url)
                    .await
                    .context("Failed to migrate database")?;
                println!("{:?}", result);
            }
        }
    }

    Ok(args)
}

const BUILD_INFO: [(&'static str, Option<&'static str>); 10] = [
    ("Build Timestamp", option_env!("VERGEN_BUILD_TIMESTAMP")),
    ("Build Version", option_env!("VERGEN_GIT_SEMVER")),
    ("Commit SHA", option_env!("VERGEN_GIT_SHA")),
    ("Commit Date", option_env!("VERGEN_GIT_COMMIT_TIMESTAMP")),
    ("Commit Branch", option_env!("VERGEN_GIT_BRANCH")),
    ("rustc Version", option_env!("VERGEN_RUSTC_SEMVER")),
    ("rustc Channel", option_env!("VERGEN_RUSTC_CHANNEL")),
    ("rustc Host Triple", option_env!("VERGEN_RUSTC_HOST_TRIPLE")),
    (
        "cargo Target Triple",
        option_env!("VERGEN_CARGO_TARGET_TRIPLE"),
    ),
    ("cargo Profile", option_env!("VERGEN_CARGO_PROFILE")),
];

fn print_version() {
    for (label, value) in BUILD_INFO {
        println!("{}: {}", label, value.unwrap_or("N/A"));
    }
}
