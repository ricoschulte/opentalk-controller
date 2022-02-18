use self::types::protocol::NewProtocol;
use crate::rooms::RoomId;
use crate::schema::legal_votes;
use crate::users::UserId;
use chrono::{DateTime, Utc};
use controller_shared::{impl_from_redis_value_de, impl_to_redis_args_se};
use database::{DatabaseError, DbInterface, Paginate, Result};
use diesel::dsl::any;
use diesel::result::DatabaseErrorKind;
use diesel::{
    Connection, ExpressionMethods, Identifiable, QueryDsl, QueryResult, Queryable, RunQueryDsl,
};
use types::protocol::Protocol;

pub mod types;

diesel_newtype! {
    #[derive(Copy)] LegalVoteId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid", "/legal_vote/",
    #[derive(Copy)] SerialLegalVoteId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt"
}

impl_to_redis_args_se!(LegalVoteId);
impl_from_redis_value_de!(LegalVoteId);

/// Diesel legal_vote struct
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

/// Diesel legal_vote struct
///
/// Represents a legal vote in the database
#[derive(Debug, Clone, Insertable)]
#[table_name = "legal_votes"]
pub struct NewLegalVote {
    pub created_by: UserId,
    pub protocol: NewProtocol,
    pub room: Option<RoomId>,
}

pub trait DbLegalVoteEx: DbInterface {
    /// Insert a [`NewLegalVote`] into the database and returns the created [`LegalVote`]
    ///
    /// Generates a [`VoteId`] (uuid) for the entry in the database. In case the insert statement fails with an `UniqueViolation`,
    /// the statement will be resend with a different [`VoteId`] to counteract uuid collisions.
    #[tracing::instrument(skip(self))]
    fn new_legal_vote(
        &self,
        room_id: RoomId,
        initiator: UserId,
        protocol_version: u8,
    ) -> Result<LegalVote> {
        let conn = self.get_conn()?;

        let protocol = NewProtocol::new(Vec::new());

        let mut last_err = None;

        // Try 3 times to generate a UUID without collision.
        // While a single collision is highly unlikely, this enforces
        // a unique ID in this sensitive topic. If this fails more than
        // 3 times, something is wrong with our randomness or our database.
        for _ in 0..3 {
            let new_legal_vote = NewLegalVote {
                created_by: initiator,
                protocol: protocol.clone(),
                room: Some(room_id),
            };

            let query_result: QueryResult<LegalVote> = conn.transaction(|| {
                let legal_vote: LegalVote = diesel::insert_into(legal_votes::table)
                    .values(new_legal_vote)
                    .get_result(&conn)?;

                Ok(legal_vote)
            });

            match query_result {
                Ok(legal_vote) => return Ok(legal_vote),
                Err(error) => {
                    if let diesel::result::Error::DatabaseError(
                        DatabaseErrorKind::UniqueViolation,
                        _,
                    ) = &error
                    {
                        log::warn!(
                            "Unique violation when creating new legal vote, {:?},- reattempting operation in case this is a uuid collision",
                            error
                        );

                        last_err = Some(error.into());
                    } else {
                        return Err(error.into());
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            DatabaseError::Custom(
                "Unable to create legal vote after 3 attempts with unique violation".into(),
            )
        }))
    }

    /// Set the vote protocol for the provided `legal_vote_id`
    #[tracing::instrument(skip(self, protocol))]
    fn set_protocol(&self, legal_vote_id: LegalVoteId, protocol: NewProtocol) -> Result<()> {
        let con = self.get_conn()?;

        let protocol =
            serde_json::to_value(protocol).map_err(|e| DatabaseError::Custom(e.to_string()))?;

        let target = legal_votes::table.filter(legal_votes::columns::id.eq(&legal_vote_id));

        diesel::update(target)
            .set(legal_votes::protocol.eq(protocol))
            .execute(&con)?;

        Ok(())
    }

    /// Get the legal vote with the provided `legal_vote_id`
    #[tracing::instrument(skip(self))]
    fn get_legal_vote(&self, legal_vote_id: LegalVoteId) -> Result<Option<LegalVote>> {
        let con = self.get_conn()?;

        let query_result: QueryResult<LegalVote> = legal_votes::table
            .filter(legal_votes::columns::id.eq(&legal_vote_id))
            .get_result(&con);

        match query_result {
            Ok(legal_vote) => Ok(Some(legal_vote)),
            Err(diesel::NotFound) => Ok(None),
            Err(e) => {
                log::error!("Query error getting legal vote by id, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self))]
    fn get_legal_votes_by_id_for_room_paginated(
        &self,
        room_id: RoomId,
        accessible_ids: &[LegalVoteId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let conn = self.get_conn()?;

        let query = legal_votes::table
            .filter(legal_votes::columns::room.eq(room_id))
            .filter(legal_votes::columns::id.eq(any(accessible_ids)))
            .paginate_by(limit, page);

        let query_result = query.load_and_count::<LegalVote, _>(&conn);

        match query_result {
            Ok(legal_votes) => Ok(legal_votes),
            Err(e) => {
                log::error!("Query error getting all legal votes by id for room, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self))]
    fn get_legal_votes_by_id_paginated(
        &self,
        ids: &[LegalVoteId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let conn = self.get_conn()?;

        let query = legal_votes::table
            .filter(legal_votes::columns::id.eq(any(ids)))
            .paginate_by(limit, page);

        let query_result = query.load_and_count::<LegalVote, _>(&conn);

        match query_result {
            Ok(legal_votes) => Ok(legal_votes),
            Err(e) => {
                log::error!("Query error getting all legal votes by id, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self))]
    fn get_all_legal_votes_paginated(
        &self,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let conn = self.get_conn()?;

        let query = legal_votes::table
            .order_by(legal_votes::columns::id.desc())
            .paginate_by(limit, page);

        let query_result = query.load_and_count::<LegalVote, _>(&conn);

        match query_result {
            Ok(legal_votes) => Ok(legal_votes),
            Err(e) => {
                log::error!("Query error getting all legal votes, {}", e);
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(skip(self))]
    fn get_all_legal_votes_for_room_paginated(
        &self,
        room_id: RoomId,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<LegalVote>, i64)> {
        let conn = self.get_conn()?;

        let query = legal_votes::table
            .filter(legal_votes::columns::room.eq(room_id))
            .paginate_by(limit, page);

        let query_result = query.load_and_count::<LegalVote, _>(&conn);

        match query_result {
            Ok(legal_votes) => Ok(legal_votes),
            Err(e) => {
                log::error!("Query error getting all legal votes, {}", e);
                Err(e.into())
            }
        }
    }
}

impl<T: DbInterface> DbLegalVoteEx for T {}
