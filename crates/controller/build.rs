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
