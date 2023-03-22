// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::internal::NoContent;
use crate::api::v1::events::associated_resource_ids;
use crate::api::v1::response::ApiError;
use crate::storage::assets::asset_key;
use crate::storage::ObjectStorage;
use actix_web::delete;
use actix_web::web::{Data, Path, ReqData};
use database::{DatabaseError, Db};
use db_storage::assets::Asset;
use db_storage::events::Event;
use db_storage::legal_votes::LegalVote;
use db_storage::rooms::Room;
use db_storage::sip_configs::SipConfig;
use db_storage::users::User;
use diesel::Connection;
use kustos::prelude::*;
use types::core::RoomId;

/// API Endpoint *DELETE /rooms/{room_id}*
///
/// Deletes the room and owned resources and linked events. This endpoint is rather complex as it
/// deletes multiple underlying REST exposed resources.
/// We need to check if we have access to all resources that need to be removed during this operation, and
/// we need to make sure to delete all related authz permissions of those resources.
///
/// We cannot rely on DB cascading as this would result in idling permissions.
///
/// Important:
/// Access checks should not be handled via a middleware but instead done inside, as this deletes multiple resources
#[delete("/rooms/{room_id}")]
pub async fn delete(
    db: Data<Db>,
    storage: Data<ObjectStorage>,
    room_id: Path<RoomId>,
    current_user: ReqData<User>,
    authz: Data<Authz>,
) -> Result<NoContent, ApiError> {
    let room_id = room_id.into_inner();
    let room_path = format!("/rooms/{room_id}");

    let db_clone = db.clone();
    let (mut linked_events, mut linked_legal_votes) =
        crate::block(move || -> database::Result<_> {
            let mut conn = db_clone.get_conn()?;

            Room::get(&mut conn, room_id)?;

            Ok((
                Event::get_all_ids_for_room(&mut conn, room_id)?,
                LegalVote::get_all_ids_for_room(&mut conn, room_id)?,
            ))
        })
        .await??;

    // Sort for improved equality comparison later on, inside the transaction.
    linked_events.sort();
    linked_legal_votes.sort();

    // Enforce access to all DELETE operations
    let mut resources = linked_events
        .iter()
        .map(|e| e.resource_id())
        .chain(linked_legal_votes.iter().map(|e| e.resource_id()))
        .collect::<Vec<_>>();

    resources.push(room_path.clone().into());

    let checked = authz
        .check_batched(current_user.id, resources.clone(), AccessMethod::DELETE)
        .await?;

    if checked.iter().any(|&res| !res) {
        return Err(ApiError::forbidden());
    }

    let resources: Vec<_> = linked_events
        .iter()
        .flat_map(|&event_id| associated_resource_ids(event_id))
        .chain(linked_legal_votes.iter().map(|e| e.resource_id()))
        .chain(associated_room_resource_ids(room_id))
        .collect();

    let assets = crate::block(move || {
        let mut conn = db.get_conn()?;
        conn.transaction(|conn| {
            // We check if in the meantime (during the permission check) another event got linked to
            let mut current_events = Event::get_all_ids_for_room(conn, room_id)?;
            current_events.sort();

            if current_events != linked_events {
                return Err(DatabaseError::custom("Race-condition during access checks"));
            }

            let mut current_legal_votes = LegalVote::get_all_ids_for_room(conn, room_id)?;
            current_legal_votes.sort();

            if current_legal_votes != linked_legal_votes {
                return Err(DatabaseError::custom("Race-condition during access checks"));
            }

            let mut current_assets = Asset::get_all_ids_for_room(conn, room_id)?;
            current_assets.sort();

            LegalVote::delete_by_room(conn, room_id)?;
            Event::delete_all_for_room(conn, room_id)?;
            SipConfig::delete_by_room(conn, room_id)?;
            Asset::delete_by_ids(conn, &current_assets)?;
            Room::delete_by_id(conn, room_id)?;

            Ok(current_assets)
        })
    })
    .await??;

    for asset_id in assets {
        storage.delete(asset_key(&asset_id)).await?;
    }

    authz.remove_explicit_resources(resources).await?;

    Ok(NoContent {})
}

pub(crate) fn associated_room_resource_ids(
    room_id: RoomId,
) -> impl IntoIterator<Item = ResourceId> {
    [
        ResourceId::from(format!("/rooms/{room_id}")),
        ResourceId::from(format!("/rooms/{room_id}/invites")),
        ResourceId::from(format!("/rooms/{room_id}/invites/*")),
        ResourceId::from(format!("/rooms/{room_id}/start")),
    ]
}
