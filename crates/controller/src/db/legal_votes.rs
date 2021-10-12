use super::{DatabaseError, Result};
use crate::db::schema::legal_votes;
use crate::db::users::UserId;
use crate::db::DbInterface;
use crate::{impl_from_redis_value_de, impl_to_redis_args_se};
use diesel::result::DatabaseErrorKind;
use diesel::{ExpressionMethods, Identifiable, QueryDsl, QueryResult, Queryable, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

diesel_newtype!(#[derive(Copy)] VoteId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid");

impl_to_redis_args_se!(VoteId);
impl_from_redis_value_de!(VoteId);

/// Diesel legal_vote struct
///
/// Represents a legal vote in the database
#[derive(Debug, Queryable, Identifiable)]
pub struct LegalVote {
    pub id: VoteId,
    pub initiator: UserId,
    pub protocol: serde_json::Value,
}

/// Diesel legal_vote struct
///
/// Represents a legal vote in the database
#[derive(Debug, Clone, Insertable)]
#[table_name = "legal_votes"]
pub struct NewLegalVote {
    pub id: VoteId,
    pub initiator: UserId,
    pub protocol: serde_json::Value,
}

impl DbInterface {
    /// Insert a [`NewLegalVote`] into the database and returns the created [`LegalVote`]
    ///
    /// Generates a [`VoteId`] (uuid) for the entry in the database. In case the insert statement fails with an `UniqueViolation`,
    /// the statement will be resend with a different [`VoteId`] to counteract uuid collisions.
    #[tracing::instrument(skip(self, protocol))]
    pub fn new_legal_vote<T: Serialize>(
        &self,
        initiator: UserId,
        protocol: T,
    ) -> Result<LegalVote> {
        let con = self.get_con()?;

        let protocol =
            serde_json::to_value(protocol).map_err(|e| DatabaseError::Error(e.to_string()))?;

        let mut last_err = None;

        // Try 3 times to generate a UUID without collision.
        // While a single collision is highly unlikely, this enforces
        // a unique ID in this sensitive topic. If this fails more than
        // 3 times, something is wrong with our randomness or our database.
        for _ in 0..3 {
            let new_legal_vote = NewLegalVote {
                id: VoteId::from(Uuid::new_v4()),
                initiator,
                protocol: protocol.clone(),
            };

            let query_result: QueryResult<LegalVote> = diesel::insert_into(legal_votes::table)
                .values(new_legal_vote)
                .get_result(&con);

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
            DatabaseError::Error(
                "Unable to create legal vote after 3 attempts with unique violation".into(),
            )
        }))
    }

    /// Set the vote protocol for the provided `vote_id`
    #[tracing::instrument(skip(self, protocol))]
    pub fn set_protocol<T: Serialize>(&self, vote_id: VoteId, protocol: T) -> Result<()> {
        let con = self.get_con()?;

        let protocol =
            serde_json::to_value(protocol).map_err(|e| DatabaseError::Error(e.to_string()))?;

        let target = legal_votes::table.filter(legal_votes::columns::id.eq(&vote_id));

        diesel::update(target)
            .set(legal_votes::protocol.eq(protocol))
            .execute(&con)?;

        Ok(())
    }

    /// Get the legal vote with the provided `vote_id`
    #[tracing::instrument(skip(self))]
    pub fn get_legal_vote(&self, vote_id: VoteId) -> Result<Option<LegalVote>> {
        let con = self.get_con()?;

        let query_result: QueryResult<LegalVote> = legal_votes::table
            .filter(legal_votes::columns::id.eq(&vote_id))
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
}
