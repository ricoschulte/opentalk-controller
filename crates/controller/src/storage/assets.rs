// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use super::ObjectStorage;
use anyhow::{Context, Result};
use aws_sdk_s3::types::ByteStream;
use bytes::Bytes;
use database::Db;
use db_storage::assets::{Asset, AssetId, NewAsset};
use db_storage::rooms::RoomId;
use futures::Stream;
use std::sync::Arc;
use uuid::Uuid;

/// Save an asset in the long term storage
///
/// Creates a new database entry before after the asset in the configured S3 bucket.
pub async fn save_asset(
    storage: &ObjectStorage,
    db: Arc<Db>,
    room_id: RoomId,
    namespace: Option<&str>,
    filename: impl Into<String>,
    kind: impl Into<String>,
    data: impl Stream<Item = Result<Bytes>> + Unpin,
) -> Result<AssetId> {
    let namespace = namespace.map(Into::into);
    let filename = filename.into();
    let kind = kind.into();

    let asset_id = AssetId::from(Uuid::new_v4());

    // upload to s3 storage
    storage
        .put(&asset_key(&asset_id), data)
        .await
        .context("failed to upload asset file to storage")?;

    // create db entry
    let block_result = crate::block(move || {
        let mut db_conn = db.get_conn()?;

        NewAsset {
            id: asset_id,
            namespace,
            filename,
            kind,
        }
        .insert_for_room(&mut db_conn, room_id)
    })
    .await;

    let mut is_error = false;

    // check possible errors
    match block_result {
        Ok(db_result) => {
            if let Err(e) = db_result {
                log::error!("Failed to create new asset in db: {}", e);
                is_error = true;
            }
        }
        Err(e) => {
            log::error!("Blocking error while creating asset: {}", e);
            is_error = true;
        }
    }

    // rollback s3 storage if errors occurred
    if is_error {
        if let Err(err) = storage.delete(asset_key(&asset_id)).await {
            log::error!(
                "Failed to rollback s3 asset after database error, leaking asset: {}",
                &asset_key(&asset_id)
            );
            return Err(err);
        }
    }

    Ok(asset_id)
}

/// Get an asset from the object storage
pub async fn get_asset(storage: &ObjectStorage, asset_id: &AssetId) -> Result<ByteStream> {
    storage.get(asset_key(asset_id)).await
}

/// Delete an asset from the object storage
pub async fn delete_asset(
    storage: &ObjectStorage,
    db: Arc<Db>,
    room_id: RoomId,
    asset_id: AssetId,
) -> Result<()> {
    crate::block(move || {
        let mut conn = db.get_conn()?;

        Asset::delete_by_id(&mut conn, asset_id, room_id)
    })
    .await??;

    storage.delete(asset_key(&asset_id)).await
}

pub fn asset_key(asset_id: &AssetId) -> String {
    format!("assets/{asset_id}")
}
