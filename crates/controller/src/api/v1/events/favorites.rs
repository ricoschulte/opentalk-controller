use crate::api::v1::response::{ApiError, Created, NoContent};
use actix_web::web::{Data, Path, ReqData};
use actix_web::{delete, put, Either};
use database::Db;
use db_storage::events::{Event, EventId};
use db_storage::users::User;

/// API Endpoint *PUT /users/me/event_favorites/{event_id}*
///
/// Add an event to the current users favorites
#[put("/users/me/event_favorites/{event_id}")]
pub async fn add_event_to_favorites(
    db: Data<Db>,
    id: Path<EventId>,
    current_user: ReqData<User>,
) -> Result<Either<Created, NoContent>, ApiError> {
    crate::block(move || {
        let conn = db.get_conn()?;

        let result = Event::favorite_by_id(&conn, id.into_inner(), current_user.id);

        match result {
            Ok(()) => Ok(Either::Left(Created)),
            Err(database::DatabaseError::DieselError(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::ForeignKeyViolation,
                info,
            ))) if matches!(
                info.constraint_name(),
                Some("event_favorites_event_id_fkey")
            ) =>
            {
                Err(ApiError::not_found())
            }
            Err(database::DatabaseError::DieselError(diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                ..,
            ))) => Ok(Either::Right(NoContent)),
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
    crate::block(move || {
        let conn = db.get_conn()?;

        Event::unfavorite_by_id(&conn, id.into_inner(), current_user.id)?;

        Ok(NoContent)
    })
    .await?
}
