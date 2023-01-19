// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api::v1::response::{ApiError, Created, NoContent};
use actix_web::web::{Data, Path, ReqData};
use actix_web::{delete, put, Either};
use database::Db;
use db_storage::events::{Event, EventFavorite, EventId, NewEventFavorite};
use db_storage::users::User;
use diesel::Connection;

/// API Endpoint *PUT /users/me/event_favorites/{event_id}*
///
/// Add an event to the current users favorites
#[put("/users/me/event_favorites/{event_id}")]
pub async fn add_event_to_favorites(
    db: Data<Db>,
    id: Path<EventId>,
    current_user: ReqData<User>,
) -> Result<Either<Created, NoContent>, ApiError> {
    let event_id = id.into_inner();

    crate::block(move || {
        let mut conn = db.get_conn()?;

        let result = conn.transaction(|conn| {
            let _event = Event::get(conn, event_id)?;

            NewEventFavorite {
                user_id: current_user.id,
                event_id,
            }
            .try_insert(conn)
        });

        match result {
            Ok(Some(_)) => Ok(Either::Left(Created)),
            Ok(None) => Ok(Either::Right(NoContent)),
            Err(e) => Err(e.into()),
        }
    })
    .await?
}

/// API Endpoint *DELETE /users/me/event_favorites/{event_id}*
///
/// Remove an event from the current users favorites
#[delete("/users/me/event_favorites/{event_id}")]
pub async fn remove_event_from_favorites(
    db: Data<Db>,
    id: Path<EventId>,
    current_user: ReqData<User>,
) -> Result<NoContent, ApiError> {
    let event_id = id.into_inner();

    crate::block(move || {
        let mut conn = db.get_conn()?;

        let existed = EventFavorite::delete_by_id(&mut conn, current_user.id, event_id)?;

        if existed {
            Ok(NoContent)
        } else {
            Err(ApiError::not_found())
        }
    })
    .await?
}
