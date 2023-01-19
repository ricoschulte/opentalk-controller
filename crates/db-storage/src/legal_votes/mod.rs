// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use self::types::protocol::NewProtocol;
use crate::rooms::RoomId;
use crate::schema::legal_votes;
use crate::users::UserId;
use chrono::{DateTime, Utc};
use database::{DatabaseError, DbConnection, Paginate, Result};
use diesel::prelude::*;
use diesel::result::DatabaseErrorKind;
use diesel::{ExpressionMethods, Identifiable, QueryDsl, Queryable, RunQueryDsl};
use types::protocol::Protocol;

pub mod types;

diesel_newtype! {
    #[derive(Copy, redis_args::ToRedisArgs, redis_args::FromRedisValue)]
    #[to_redis_args(serde)]
    #[from_redis_value(serde)]
    LegalVoteId(uuid::Uuid) => diesel::sql_types::Uuid, "/legal_vote/",

    #[derive(Copy)]
    SerialLegalVoteId(i64) => diesel::sql_types::BigInt
}

/// Diesel legal_vote model
///
/// Represents a legal vote in the database
#[derive(Debug, Queryable, Identifiable)]
pub struct LegalVote {
    pub id: LegalVoteId,
    pub id_serial: SerialLegalVoteId,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    pub room: Option<RoomId>,
    pub protocol: Protocol,
}

impl LegalVote {
    /// Get the `LegalVote` with the provided `legal_vote_id`
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &mut DbConnection, legal_vote_id: LegalVoteId) -> Result<LegalVote> {
        let query = legal_votes::table.filter(legal_votes::id.eq(legal_vote_id));

        let legal_vote = query.get_result(conn)?;

        Ok(legal_vote)
    }

    /// Get all `LegalVote` filtered by ids and room, paginated
    #[tracing::instrument(err, skip_all)]
    pub fn get_for_room_by_ids_paginated(
        conn: &mut DbConnection,
        room_id: RoomId,
        accessible_ids: &[LegalVoteId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let query = legal_votes::table
            .filter(legal_votes::room.eq(room_id))
            .filter(legal_votes::id.eq_any(accessible_ids))
            .paginate_by(limit, page);

        let legal_votes_with_total = query.load_and_count::<LegalVote, _>(conn)?;

        Ok(legal_votes_with_total)
    }

    /// Get all `LegalVote`s by ids, paginated
    #[tracing::instrument(err, skip_all)]
    pub fn get_by_ids_paginated(
        conn: &mut DbConnection,
        ids: &[LegalVoteId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let query = legal_votes::table
            .filter(legal_votes::id.eq_any(ids))
            .paginate_by(limit, page);

        let legal_votes_with_total = query.load_and_count::<LegalVote, _>(conn)?;

        Ok(legal_votes_with_total)
    }

    /// Get all `LegalVote`s, paginated
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_paginated(
        conn: &mut DbConnection,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let query = legal_votes::table
            .order_by(legal_votes::id.desc())
            .paginate_by(limit, page);

        let legal_votes_with_total = query.load_and_count::<LegalVote, _>(conn)?;

        Ok(legal_votes_with_total)
    }

    /// Get all `LegalVotes` for room, paginated
    #[tracing::instrument(err, skip_all)]
    pub fn get_for_room_paginated(
        conn: &mut DbConnection,
        room_id: RoomId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let query = legal_votes::table
            .filter(legal_votes::room.eq(room_id))
            .paginate_by(limit, page);

        let legal_votes_with_total = query.load_and_count::<LegalVote, _>(conn)?;

        Ok(legal_votes_with_total)
    }

    /// Get all `LegalVotes` for room
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_ids_for_room(
        conn: &mut DbConnection,
        room_id: RoomId,
    ) -> Result<Vec<LegalVoteId>> {
        let query = legal_votes::table
            .select(legal_votes::id)
            .filter(legal_votes::room.eq(room_id));

        let legal_votes_with_total = query.load(conn)?;

        Ok(legal_votes_with_total)
    }

    /// Delete all `LegalVotes` for room
    #[tracing::instrument(err, skip_all)]
    pub fn delete_by_room(conn: &mut DbConnection, room_id: RoomId) -> Result<()> {
        diesel::delete(legal_votes::table)
            .filter(legal_votes::room.eq(room_id))
            .execute(conn)?;

        Ok(())
    }
}

/// LegalVote insert values
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = legal_votes)]
pub struct NewLegalVote {
    pub created_by: UserId,
    pub protocol: NewProtocol,
    pub room: Option<RoomId>,
}

impl NewLegalVote {
    pub fn new(created_by: UserId, room_id: RoomId) -> Self {
        Self {
            created_by,
            protocol: NewProtocol::new(Vec::new()),
            room: Some(room_id),
        }
    }

    /// Insert a [`NewLegalVote`] into the database and returns the created [`LegalVote`]
    ///
    /// Generates a [`LegalVoteId`] (uuid) for the entry in the database. In case the insert statement fails with an `UniqueViolation`,
    /// the statement will be resend with a different [`LegalVoteId`] to counteract uuid collisions.
    #[tracing::instrument(err, skip_all)]
    pub fn insert(self, conn: &mut DbConnection) -> Result<LegalVote> {
        // Try 3 times to generate a UUID without collision.
        // While a single collision is highly unlikely, this enforces
        // a unique ID in this sensitive topic. If this fails more than
        // 3 times, something is wrong with our randomness or our database.
        for _ in 0..3 {
            let query = self.clone().insert_into(legal_votes::table);

            match query.get_result(conn) {
                Ok(legal_vote) => return Ok(legal_vote),
                Err(diesel::result::Error::DatabaseError(
                    DatabaseErrorKind::UniqueViolation,
                    _,
                )) => {
                    log::warn!(
                        "Unique violation when creating new legal vote,- reattempting operation in case this is a uuid collision",
                    );
                }
                Err(e) => return Err(e.into()),
            }
        }

        Err(DatabaseError::custom(
            "Unable to create legal vote after 3 attempts with unique violation",
        ))
    }
}

/// Set the vote protocol for the provided `legal_vote_id`
// FIXME(r.floren): Is there a mentally better place for this? LegalVote does not make much sense, and NewProtocol neither.
#[tracing::instrument(err, skip_all)]
pub fn set_protocol(
    conn: &mut DbConnection,
    legal_vote_id: LegalVoteId,
    protocol: NewProtocol,
) -> Result<()> {
    let protocol =
        serde_json::to_value(protocol).map_err(|e| DatabaseError::Custom(e.to_string().into()))?;

    let query = diesel::update(legal_votes::table.filter(legal_votes::id.eq(&legal_vote_id)))
        .set(legal_votes::protocol.eq(protocol));

    query.execute(conn)?;

    Ok(())
}
