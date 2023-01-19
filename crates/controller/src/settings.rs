// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Handles the application settings via a config file and environment variables.
use crate::cli::Args;
use actix_web::web::Data;
use arc_swap::ArcSwap;
use config::ConfigError;
use std::sync::Arc;

pub use controller_shared::settings::*;

pub type SharedSettingsActix = Data<ArcSwap<Settings>>;

/// Reload the settings from the `config_path` & the environment
///
/// Not all settings are used, as most of the settings are not reloadable while the
/// controller is running.
pub(crate) fn reload_settings(
    shared_settings: SharedSettings,
    config_path: &str,
) -> Result<(), ConfigError> {
    let new_settings = Settings::load(config_path)?;
    let mut current_settings = (*shared_settings.load_full()).clone();

    // reload extensions config
    current_settings.extensions = new_settings.extensions;

    // reload turn settings
    current_settings.turn = new_settings.turn;

    // reload metrics
    current_settings.metrics = new_settings.metrics;

    // reload avatar
    current_settings.avatar = new_settings.avatar;

    // reload call in
    current_settings.call_in = new_settings.call_in;

    // replace the shared settings with the modified ones
    shared_settings.store(Arc::new(current_settings));

    Ok(())
}

/// Loads settings from program arguments and config file
///
/// The settings specified in the CLI-Arguments have a higher priority than the settings specified in the config file
pub fn load_settings(args: &Args) -> Result<Settings, ConfigError> {
    Settings::load(&args.config)
}

#[cfg(test)]
mod test {
    use config::ConfigError;
    use controller_shared::settings::Settings;

    #[test]
    fn example_toml() -> Result<(), ConfigError> {
        Settings::load("../../extra/example.toml")?;
        Ok(())
    }
}
