// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::schema::{tenants, users};
use crate::users::UserId;
use chrono::{DateTime, Utc};
use database::{DbConnection, Result};
use diesel::prelude::*;
use types::core::TenantId;

types::diesel_newtype! {
    OidcTenantId(String) => diesel::sql_types::Text
}

#[derive(Debug, Clone, Queryable, Identifiable)]
pub struct Tenant {
    pub id: TenantId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub oidc_tenant_id: OidcTenantId,
}

impl Tenant {
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &mut DbConnection, id: TenantId) -> Result<Tenant> {
        let query = tenants::table.filter(tenants::id.eq(id));
        let tenant = query.get_result(conn)?;
        Ok(tenant)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_by_oidc_id(conn: &mut DbConnection, id: OidcTenantId) -> Result<Option<Tenant>> {
        let query = tenants::table.filter(tenants::oidc_tenant_id.eq(id));
        let tenant = query.get_result(conn).optional()?;
        Ok(tenant)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_for_user(conn: &mut DbConnection, user_id: UserId) -> Result<Tenant> {
        let query = users::table
            .inner_join(tenants::table)
            .filter(users::id.eq(user_id))
            .select(tenants::all_columns);

        let tenant = query.get_result(conn)?;

        Ok(tenant)
    }

    pub fn get_all(conn: &mut DbConnection) -> Result<Vec<Tenant>> {
        let tenants = tenants::table.load(conn)?;
        Ok(tenants)
    }
}

#[derive(Clone, Insertable)]
#[diesel(table_name = tenants)]
pub struct NewTenant<'n> {
    pub oidc_tenant_id: &'n OidcTenantId,
}

/// Get or create a tenant by name
pub fn get_or_create_tenant_by_oidc_id(
    conn: &mut DbConnection,
    oidc_tenant_id: &OidcTenantId,
) -> Result<Tenant> {
    let present_tenant: Option<Tenant> = tenants::table
        .select(tenants::all_columns)
        .filter(tenants::oidc_tenant_id.eq(oidc_tenant_id))
        .get_result(conn)
        .optional()?;

    if let Some(tenant) = present_tenant {
        Ok(tenant)
    } else {
        let new_tenant = NewTenant { oidc_tenant_id };

        let new_tenant: Tenant = diesel::insert_into(tenants::table)
            .values(new_tenant)
            .returning(tenants::all_columns)
            .get_result(conn)?;

        Ok(new_tenant)
    }
}

#[derive(AsChangeset)]
#[diesel(table_name = tenants)]
pub struct UpdateTenant<'a> {
    pub updated_at: DateTime<Utc>,
    pub oidc_tenant_id: &'a OidcTenantId,
}

impl UpdateTenant<'_> {
    pub fn apply(self, conn: &mut DbConnection, tenant_id: TenantId) -> Result<Tenant> {
        let query = diesel::update(tenants::table.filter(tenants::id.eq(tenant_id))).set(self);
        let tenant: Tenant = query.get_result(conn)?;
        Ok(tenant)
    }
}
