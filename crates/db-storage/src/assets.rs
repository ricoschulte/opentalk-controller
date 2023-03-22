// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::schema::assets;
use crate::schema::room_assets;
use crate::tenants::TenantId;
use chrono::{DateTime, Utc};
use database::DbConnection;
use database::Paginate;
use database::Result;
use diesel::BoolExpressionMethods;
use diesel::Connection;
use diesel::Insertable;
use diesel::JoinOnDsl;
use diesel::RunQueryDsl;
use diesel::{ExpressionMethods, QueryDsl};
use diesel::{Identifiable, Queryable};
use types::core::{AssetId, RoomId};

/// Diesel resource struct
#[derive(Debug, Clone, Queryable, Identifiable)]
pub struct Asset {
    pub id: AssetId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub namespace: Option<String>,
    pub kind: String,
    pub filename: String,
    pub tenant_id: TenantId,
}

impl Asset {
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &mut DbConnection, id: AssetId, room_id: RoomId) -> Result<Self> {
        //FIXME: The inner_join below (as well as the room_id parameter) can be removed when assets have their own
        // permission check and don't rely on room permissions

        let query = assets::table
            .inner_join(
                room_assets::table.on(room_assets::asset_id
                    .eq(assets::id)
                    .and(room_assets::room_id.eq(room_id))),
            )
            .filter(assets::id.eq(id))
            .select(assets::all_columns);

        let resource: Asset = query.get_result(conn)?;

        Ok(resource)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_all_ids_for_room(conn: &mut DbConnection, room_id: RoomId) -> Result<Vec<AssetId>> {
        let query = room_assets::table
            .select(room_assets::asset_id)
            .filter(room_assets::room_id.eq(room_id));

        let assets = query.load(conn)?;

        Ok(assets)
    }
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_for_room_paginated(
        conn: &mut DbConnection,
        room_id: RoomId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Self>, i64)> {
        let query = assets::table
            .inner_join(room_assets::table.on(room_assets::asset_id.eq(assets::id)))
            .filter(room_assets::room_id.eq(room_id))
            .select(assets::all_columns)
            .paginate_by(limit, page);

        let resources_with_total = query.load_and_count(conn)?;

        Ok(resources_with_total)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_all_for_rooms_paginated(
        conn: &mut DbConnection,
        room_ids: &[RoomId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Self>, i64)> {
        let query = assets::table
            .inner_join(room_assets::table.on(room_assets::asset_id.eq(assets::id)))
            .filter(room_assets::room_id.eq_any(room_ids))
            .select(assets::all_columns)
            .paginate_by(limit, page);

        let resources_with_total = query.load_and_count(conn)?;

        Ok(resources_with_total)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn get_all_paginated(
        conn: &mut DbConnection,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<Self>, i64)> {
        let query = assets::table
            .select(assets::all_columns)
            .paginate_by(limit, page);

        let resources_with_total = query.load_and_count(conn)?;

        Ok(resources_with_total)
    }

    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_id(conn: &mut DbConnection, asset_id: AssetId, room_id: RoomId) -> Result<()> {
        conn.transaction(|conn| {
            //FIXME: This check (as well as the room_id parameter) can be removed when assets have their own permission
            // check and don't rely on room permissions
            //
            // check if the asset exists for the specified room
            room_assets::table
                .filter(
                    room_assets::asset_id
                        .eq(asset_id)
                        .and(room_assets::room_id.eq(room_id)),
                )
                .execute(conn)?;

            diesel::delete(assets::table.filter(assets::id.eq(asset_id))).execute(conn)?;

            Ok(())
        })
    }

    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_ids(conn: &mut DbConnection, asset_ids: &[AssetId]) -> Result<()> {
        let query = diesel::delete(assets::table.filter(assets::id.eq_any(asset_ids)));

        query.execute(conn)?;

        Ok(())
    }
}

#[derive(Debug, Insertable)]
pub struct RoomAsset {
    pub room_id: RoomId,
    pub asset_id: AssetId,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = assets)]
pub struct NewAsset {
    pub id: AssetId,
    pub namespace: Option<String>,
    pub kind: String,
    pub filename: String,
    pub tenant_id: TenantId,
}

impl NewAsset {
    #[tracing::instrument(err, skip_all)]
    pub fn insert_for_room(self, conn: &mut DbConnection, room_id: RoomId) -> Result<Asset> {
        conn.transaction(|conn| {
            let asset: Asset = self.insert_into(assets::table).get_result(conn)?;

            RoomAsset {
                room_id,
                asset_id: asset.id,
            }
            .insert_into(room_assets::table)
            .execute(conn)?;

            Ok(asset)
        })
    }
}
