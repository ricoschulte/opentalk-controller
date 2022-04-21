// Copyright (C) 2022 OpenTalk GmbH
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use anyhow::Result;
use vergen::{vergen, Config};

fn main() -> Result<()> {
    let config = if !git_at_cd_or_above()? {
        // Setup your config without git enabled
        let mut config = Config::default();
        *config.git_mut().enabled_mut() = false;
        config
    } else {
        // Setup your config with git enabled
        Config::default()
    };

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
