use anyhow::{Context, Result};
use clap::{ArgAction, Parser, Subcommand};
use controller_shared::settings::Settings;

mod acl;
mod fix_acl;
mod reload;

#[derive(Parser, Debug, Clone)]
#[clap(name = "k3k-controller")]
pub struct Args {
    #[clap(
        short,
        long,
        default_value = "config.toml",
        help = "Specify path to configuration file"
    )]
    pub config: String,

    /// Triggers a reload of the Janus Server configuration
    #[clap(long)]
    pub reload: bool,

    #[clap(subcommand)]
    cmd: Option<SubCommand>,

    #[clap(long, action=ArgAction::SetTrue)]
    version: bool,
}

#[derive(Subcommand, Debug, Clone)]
#[clap(rename_all = "kebab_case")]
enum SubCommand {
    /// Rebuild ACLs based on current data
    FixAcl {
        /// Do not add user roles
        #[clap(long = "no-user-roles", parse(from_flag = std::ops::Not::not))]
        user_roles: bool,
        /// Do not add user groups
        #[clap(long = "no-user-groups", parse(from_flag = std::ops::Not::not))]
        user_groups: bool,
        /// Do not add room owner read/write access
        #[clap(long = "no-room-creators", parse(from_flag = std::ops::Not::not))]
        room_creators: bool,
    },
    /// Modify the ACLs.
    #[clap(subcommand)]
    Acl(AclSubCommand),
    /// Migrate the db. This is done automatically during start of the controller,
    /// but can be done without starting the controller using this command.
    MigrateDb,
}

#[derive(Subcommand, Debug, Clone)]
#[clap(rename_all = "kebab_case")]
pub(crate) enum AclSubCommand {
    /// Allows all users access to all rooms
    UsersHaveAccessToAllRooms {
        /// Enable/Disable
        #[clap(subcommand)]
        action: EnableDisable,
    },
}

#[derive(Parser, Debug, Clone)]
#[clap(rename_all = "kebab_case")]
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

const BUILD_INFO: [(&str, Option<&str>); 10] = [
    ("Build Timestamp", option_env!("VERGEN_BUILD_TIMESTAMP")),
    ("Build Version", option_env!("VERGEN_BUILD_SEMVER")),
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
