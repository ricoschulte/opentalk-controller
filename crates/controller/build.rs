// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::Result;
use vergen::{vergen, Config};

fn add_custom_environment_variable(name: &str, config_flag: &mut bool) {
    if let Ok(s) = std::env::var(name) {
        *config_flag = false;
        println!("cargo:rustc-env={}={}", name, s);
    }
    println!("cargo:rerun-if-env-changed={}", name);
}

fn main() -> Result<()> {
    let mut config = if !git_at_cd_or_above()? {
        // Setup your config without git disabled
        let mut config = Config::default();
        *config.git_mut().enabled_mut() = false;
        config
    } else {
        // Setup your config with git enabled
        Config::default()
    };

    add_custom_environment_variable("VERGEN_BUILD_TIMESTAMP", config.build_mut().timestamp_mut());
    add_custom_environment_variable("VERGEN_GIT_SHA", config.git_mut().sha_mut());
    add_custom_environment_variable(
        "VERGEN_GIT_COMMIT_TIMESTAMP",
        config.git_mut().commit_timestamp_mut(),
    );
    add_custom_environment_variable("VERGEN_GIT_BRANCH", config.git_mut().branch_mut());

    // Generate the default 'cargo:' instruction output
    vergen(config)
}

fn git_at_cd_or_above() -> Result<bool> {
    let current_dir = std::env::current_dir()?;
    let mut parents = vec![];
    let mut path = &*current_dir.canonicalize()?;
    while let Some(parent) = path.parent() {
        parents.push(parent);
        path = parent;
    }
    for parent in parents.into_iter().rev() {
        if parent.join(".git").exists() {
            return Ok(true);
        }
    }
    Ok(false)
}
