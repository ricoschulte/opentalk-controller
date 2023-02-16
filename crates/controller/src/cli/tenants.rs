// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Subcommand;
use controller_shared::settings::Settings;
use database::Db;
use db_storage::tenants::{OidcTenantId, Tenant, TenantId, UpdateTenant};
use tabled::{Style, Table, Tabled};
use uuid::Uuid;

#[derive(Subcommand, Debug, Clone)]
#[clap(rename_all = "kebab_case")]
pub enum Command {
    /// List all available tenants
    List,
    /// Change a tenants oidc-id
    SetOidcId { id: Uuid, new_oidc_id: String },
}

pub fn handle_command(settings: Settings, command: Command) -> Result<()> {
    match command {
        Command::List => list_all_tenants(settings),
        Command::SetOidcId { id, new_oidc_id } => set_oidc_id(
            settings,
            TenantId::from(id),
            OidcTenantId::from(new_oidc_id),
        ),
    }
}

#[derive(Tabled)]
struct TenantTableRow {
    id: TenantId,
    oidc_id: OidcTenantId,
}

impl TenantTableRow {
    fn from_tenant(tenant: Tenant) -> Self {
        Self {
            id: tenant.id,
            oidc_id: tenant.oidc_tenant_id,
        }
    }
}

/// Implementation of the `k3k-controller tenants list` command
fn list_all_tenants(settings: Settings) -> Result<()> {
    let db = Db::connect(&settings.database).context("Failed to connect to database")?;
    let mut conn = db.get_conn()?;

    let tenants = Tenant::get_all(&mut conn)?;
    let rows: Vec<TenantTableRow> = tenants
        .into_iter()
        .map(TenantTableRow::from_tenant)
        .collect();

    println!("{}", Table::new(rows).with(Style::psql()));

    Ok(())
}

/// Implementation of the `k3k-controller tenants set-oidc-id <tenant-id> <new-oidc-id>` command
fn set_oidc_id(settings: Settings, id: TenantId, new_oidc_id: OidcTenantId) -> Result<()> {
    let db = Db::connect(&settings.database).context("Failed to connect to database")?;
    let mut conn = db.get_conn()?;

    let tenant = Tenant::get(&mut conn, id)?;
    let old_oidc_id = tenant.oidc_tenant_id;

    UpdateTenant {
        updated_at: Utc::now(),
        oidc_tenant_id: &new_oidc_id,
    }
    .apply(&mut conn, id)?;

    println!(
        "Updated tenant's oidc-id\n\tid  = {id}\n\told = {old_oidc_id}\n\tnew = {new_oidc_id}"
    );

    Ok(())
}
