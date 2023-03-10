// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Subcommand;
use controller_shared::settings::Settings;
use database::{Db, DbConnection};
use db_storage::tariffs::{ExternalTariff, ExternalTariffId, NewTariff, Tariff, UpdateTariff};
use db_storage::utils::Jsonb;
use diesel::Connection;
use itertools::Itertools;
use std::collections::HashMap;
use tabled::{Style, Table, Tabled};

#[derive(Subcommand, Debug, Clone)]
#[clap(rename_all = "kebab_case")]
pub enum Command {
    /// List all available tariffs
    List,
    /// Create a new tariff
    Create {
        /// Name of the tariff
        tariff_name: String,
        /// Eternal ID to map to the tariff
        external_tariff_id: String,
        /// Comma-separated list of modules to disable
        #[clap(long, value_delimiter = ',')]
        disabled_modules: Vec<String>,
        /// Comma-separated list of key=value pairs
        #[clap(long, value_delimiter = ',', value_parser = parse_quota)]
        quotas: Vec<(String, u32)>,
    },
    /// Delete a tariff by name
    Delete {
        /// Name of the tariff to delete
        tariff_name: String,
    },

    /// Modify an existing tariff
    Edit {
        /// Name of the tariff to modify
        tariff_name: String,

        /// Set a new name
        #[clap(long)]
        set_name: Option<String>,

        /// Comma-separated list of external tariff_ids to add
        #[clap(long, value_delimiter = ',')]
        add_external_tariff_ids: Vec<String>,

        /// Comma-separated list of external tariff_ids to remove
        #[clap(long, value_delimiter = ',')]
        remove_external_tariff_ids: Vec<String>,

        /// Comma-separated list of module names to add
        #[clap(long, value_delimiter = ',')]
        add_disabled_modules: Vec<String>,

        /// Comma-separated list of module names to remove
        #[clap(long, value_delimiter = ',')]
        remove_disabled_modules: Vec<String>,

        /// Comma-separated list of key=value pairs to add, overwrites quotas with the same name
        #[clap(long, value_delimiter = ',', value_parser = parse_quota)]
        add_quotas: Vec<(String, u32)>,

        /// Comma-separated list of quota keys to remove
        #[clap(long, value_delimiter = ',')]
        remove_quotas: Vec<String>,
    },
}

fn parse_quota(s: &str) -> Result<(String, u32)> {
    let (name, value) = s.split_once('=').context("invalid key=value pair")?;
    let value = value
        .trim()
        .parse()
        .context("invalid quota value, expected u32")?;
    Ok((name.trim().into(), value))
}

pub fn handle_command(settings: Settings, command: Command) -> Result<()> {
    match command {
        Command::List => list_all_tariffs(settings),
        Command::Create {
            tariff_name,
            external_tariff_id,
            disabled_modules,
            quotas,
        } => create_tariff(
            settings,
            tariff_name,
            external_tariff_id,
            disabled_modules,
            quotas.into_iter().collect(),
        ),
        Command::Delete { tariff_name } => delete_tariff(settings, tariff_name),
        Command::Edit {
            tariff_name,
            set_name,
            add_external_tariff_ids,
            remove_external_tariff_ids,
            add_disabled_modules,
            remove_disabled_modules,
            add_quotas,
            remove_quotas,
        } => edit_tariff(
            settings,
            tariff_name,
            set_name,
            add_external_tariff_ids,
            remove_external_tariff_ids,
            add_disabled_modules,
            remove_disabled_modules,
            add_quotas.into_iter().collect(),
            remove_quotas,
        ),
    }
}

fn list_all_tariffs(settings: Settings) -> Result<()> {
    let db = Db::connect(&settings.database).context("Failed to connect to database")?;
    let mut conn = db.get_conn()?;

    let tariffs = Tariff::get_all(&mut conn)?;

    print_tariffs(&mut conn, tariffs)
}

fn create_tariff(
    settings: Settings,
    name: String,
    external_tariff_id: String,
    disabled_modules: Vec<String>,
    quotas: HashMap<String, u32>,
) -> Result<()> {
    let db = Db::connect(&settings.database).context("Failed to connect to database")?;
    let mut conn = db.get_conn()?;

    conn.transaction(|conn| {
        let tariff = NewTariff {
            name: name.clone(),
            quotas: Jsonb(quotas),
            disabled_modules,
        }
        .insert(conn)?;

        ExternalTariff {
            external_id: ExternalTariffId::from(external_tariff_id.clone()),
            tariff_id: tariff.id,
        }
        .insert(conn)?;

        println!(
            "Created tariff name={name:?} with external external_tariff_id={external_tariff_id:?} ({})",
            tariff.id
        );

        Ok(())
    })
}

fn delete_tariff(settings: Settings, name: String) -> Result<()> {
    let db = Db::connect(&settings.database).context("Failed to connect to database")?;
    let mut conn = db.get_conn()?;

    conn.transaction(|conn| {
        let tariff = Tariff::get_by_name(conn, &name)?;
        ExternalTariff::delete_all_for_tariff(conn, tariff.id)?;
        Tariff::delete_by_id(conn, tariff.id)?;

        println!("Deleted tariff name={name:?} ({})", tariff.id);

        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
fn edit_tariff(
    settings: Settings,
    name: String,
    set_name: Option<String>,
    add_external_tariff_ids: Vec<String>,
    remove_external_tariff_ids: Vec<String>,
    add_disabled_modules: Vec<String>,
    remove_disabled_modules: Vec<String>,
    add_quotas: HashMap<String, u32>,
    remove_quotas: Vec<String>,
) -> Result<()> {
    let db = Db::connect(&settings.database).context("Failed to connect to database")?;
    let mut conn = db.get_conn()?;

    conn.transaction(|conn| {
        let tariff = Tariff::get_by_name(conn, &name)?;

        // Remove all specified external tariff ids
        if !remove_external_tariff_ids.is_empty() {
            let external_tariff_ids_to_remove: Vec<ExternalTariffId> = remove_external_tariff_ids
                .into_iter()
                .map(ExternalTariffId::from)
                .collect();
            ExternalTariff::delete_all_for_tariff_by_external_id(
                conn,
                tariff.id,
                &external_tariff_ids_to_remove,
            )?;
        }

        // Add all specified external tariff ids
        if !add_external_tariff_ids.is_empty() {
            for to_add in add_external_tariff_ids {
                ExternalTariff {
                    external_id: ExternalTariffId::from(to_add.clone()),
                    tariff_id: tariff.id,
                }
                .insert(conn)
                .with_context(|| format!("Failed to add external tariff_id {to_add:?}"))?;
            }
        }

        // Modify the `disabled_modules` list
        let mut disabled_modules = tariff.disabled_modules;
        disabled_modules
            .retain(|disabled_module| !remove_disabled_modules.contains(disabled_module));
        disabled_modules.extend(add_disabled_modules);
        disabled_modules.sort_unstable();
        disabled_modules.dedup();

        // Modify the `quotas` set
        let mut quotas = tariff.quotas.0;
        quotas.retain(|key, _| !remove_quotas.contains(key));
        quotas.extend(add_quotas);

        // Apply changeset
        let tariff = UpdateTariff {
            name: set_name,
            updated_at: Utc::now(),
            quotas: Some(Jsonb(quotas)),
            disabled_modules: Some(disabled_modules),
        }
        .apply(conn, tariff.id)?;

        println!("Updated tariff name={:?} ({})", tariff.name, tariff.id);
        print_tariffs(conn, [tariff])
    })
}

/// Print the list of tariffs as table
fn print_tariffs(conn: &mut DbConnection, tariffs: impl IntoIterator<Item = Tariff>) -> Result<()> {
    #[derive(Tabled)]
    struct TariffTableRow {
        #[tabled(rename = "name (internal)")]
        name: String,
        #[tabled(rename = "external tariff_id")]
        ext: String,
        #[tabled(rename = "disabled modules")]
        disabled_modules: String,
        quotas: String,
    }

    let rows: Result<Vec<TariffTableRow>> = tariffs
        .into_iter()
        .map(|tariff| {
            let ids = ExternalTariff::get_all_for_tariff(conn, tariff.id)?;
            let mut ids = ids
                .into_iter()
                .map(|ext_tariff_id| ext_tariff_id.into_inner())
                .join("\n");
            if ids.is_empty() {
                ids = "-".into();
            }

            let mut disabled_modules = tariff.disabled_modules.into_iter().join("\n");
            if disabled_modules.is_empty() {
                disabled_modules = "-".into();
            }

            let mut quotas = tariff
                .quotas
                .0
                .into_iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .join("\n");
            if quotas.is_empty() {
                quotas = "-".into();
            }

            Ok(TariffTableRow {
                name: tariff.name,
                ext: ids,
                disabled_modules,
                quotas,
            })
        })
        .collect();

    println!("{}", Table::new(rows?).with(Style::ascii()));

    Ok(())
}
