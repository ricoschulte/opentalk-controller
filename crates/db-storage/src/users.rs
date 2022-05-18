//! Contains the user specific database structs amd queries
use super::groups::{Group, UserGroupRelation};
use super::schema::{groups, user_groups, users};
use crate::groups::{GroupId, NewGroup, NewUserGroupRelation};
use crate::{levenshtein, lower, soundex};
use controller_shared::{impl_from_redis_value, impl_to_redis_args};
use database::{DatabaseError, DbConnection, Paginate, Result};
use diesel::{
    BelongingToDsl, BoolExpressionMethods, Connection, ExpressionMethods, GroupedBy, Identifiable,
    Insertable, QueryDsl, Queryable, RunQueryDsl, TextExpressionMethods,
};
use kustos::subject::PolicyUser;
use std::fmt;

diesel_newtype! {
    #[derive(Copy)] SerialUserId(i64) => diesel::sql_types::BigInt, "diesel::sql_types::BigInt",
    #[derive(Copy)] UserId(uuid::Uuid) => diesel::sql_types::Uuid, "diesel::sql_types::Uuid", "/users/"
}

impl_to_redis_args!(UserId);
impl_from_redis_value!(UserId);

impl From<UserId> for PolicyUser {
    fn from(id: UserId) -> Self {
        Self::from(id.into_inner())
    }
}

/// Diesel user struct
///
/// Is used as a result in various queries. Represents a user column
#[derive(Clone, Queryable, Identifiable)]
pub struct User {
    pub id: UserId,
    pub id_serial: SerialUserId,
    pub oidc_sub: String,
    // TODO make this non-null
    pub oidc_issuer: Option<String>,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub id_token_exp: i64,
    pub language: String,
    pub display_name: String,
    pub dashboard_theme: String,
    pub conference_theme: String,
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("first_name", &self.firstname)
            .field("last_name", &self.lastname)
            .finish()
    }
}

impl User {
    /// Get a user with the given id
    #[tracing::instrument(err, skip_all)]
    pub fn get(conn: &DbConnection, user_id: UserId) -> Result<User> {
        let user = users::table
            .filter(users::id.eq(user_id))
            .get_result(conn)?;

        Ok(user)
    }

    /// Get all users alongside their current groups
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_with_groups(conn: &DbConnection) -> Result<Vec<(User, Vec<Group>)>> {
        let users_query = users::table.order_by(users::id.desc());
        let users = users_query.load(conn)?;

        let groups_query = UserGroupRelation::belonging_to(&users).inner_join(groups::table);
        let groups: Vec<Vec<(UserGroupRelation, Group)>> = groups_query
            .load::<(UserGroupRelation, Group)>(conn)?
            .grouped_by(&users);

        let users_with_groups = users
            .into_iter()
            .zip(groups)
            .map(|(user, groups)| (user, groups.into_iter().map(|(_, group)| group).collect()))
            .collect();

        Ok(users_with_groups)
    }

    /// Get all users paginated
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    pub fn get_all_paginated(
        conn: &DbConnection,
        limit: i64,
        page: i64,
    ) -> Result<(Vec<User>, i64)> {
        let query = users::table
            .order_by(users::id.desc())
            .paginate_by(limit, page);

        let users_with_total = query.load_and_count(conn)?;

        Ok(users_with_total)
    }

    /// Get Users paginated and filtered by ids
    #[tracing::instrument(err, skip_all, fields(%limit, %page))]
    pub fn get_by_ids_paginated(
        conn: &DbConnection,
        ids: &[UserId],
        limit: i64,
        page: i64,
    ) -> Result<(Vec<User>, i64)> {
        let query = users::table
            .filter(users::id.eq_any(ids))
            .order_by(users::id.desc())
            .paginate_by(limit, page);

        let users_with_total = query.load_and_count::<User, _>(conn)?;

        Ok(users_with_total)
    }

    /// Returns all `User`s filtered by id
    #[tracing::instrument(err, skip_all)]
    pub fn get_all_by_ids(conn: &DbConnection, ids: &[UserId]) -> Result<Vec<User>> {
        let query = users::table.filter(users::id.eq_any(ids));
        let users = query.load(conn)?;

        Ok(users)
    }

    /// Get user with the given issuer and sub
    #[tracing::instrument(err, skip_all)]
    pub fn get_by_oidc_sub(conn: &DbConnection, issuer: &str, sub: &str) -> Result<User> {
        let user = users::table
            .filter(users::oidc_issuer.eq(issuer).and(users::oidc_sub.eq(sub)))
            .get_result(conn)?;

        Ok(user)
    }

    /// Find users by search string
    ///
    /// This looks for similarities of the search_str in the display_name, first+lastname and email
    #[tracing::instrument(err, skip_all)]
    pub fn find(conn: &DbConnection, search_str: &str) -> Result<Vec<User>> {
        // IMPORTANT: lowercase it to match the index of the db and
        // remove all existing % in name and to avoid manipulation of the LIKE query.
        let search_str = search_str.replace('%', "").trim().to_lowercase();

        if search_str.is_empty() {
            return Ok(vec![]);
        }

        let like_query = format!("%{}%", search_str);

        let lower_display_name = lower(users::display_name);

        let lower_first_lastname = lower(users::firstname.concat(" ").concat(users::lastname));

        let matches = users::table
            .filter(
                // First try LIKE query on display_name
                lower_display_name.like(&like_query).or(
                    // Then try LIKE query with first+last name
                    lower_first_lastname
                        .like(&like_query)
                        // Then try LIKE query on email
                        .or(lower(users::email).like(&like_query))
                        //
                        // Then SOUNDEX on display_name
                        .or(soundex(lower_display_name)
                            .eq(soundex(&search_str))
                            // only take SOUNDEX results with a levenshtein score of lower than 5
                            .and(levenshtein(lower_display_name, &search_str).lt(5)))
                        //
                        // Then SOUNDEX on first+last name
                        .or(soundex(lower_first_lastname)
                            .eq(soundex(&search_str))
                            // only take SOUNDEX results with a levenshtein score of lower than 5
                            .and(levenshtein(lower_first_lastname, &search_str).lt(5))),
                ),
            )
            .order_by(levenshtein(lower_display_name, &search_str))
            .then_order_by(levenshtein(lower_first_lastname, &search_str))
            .then_order_by(users::id)
            .limit(5)
            .load(conn)?;

        Ok(matches)
    }
}
/// Diesel insertable user struct
///
/// Represents fields that have to be provided on user insertion.
#[derive(Insertable)]
#[table_name = "users"]
pub struct NewUser {
    pub oidc_sub: String,
    pub oidc_issuer: String,
    pub email: String,
    pub title: String,
    pub firstname: String,
    pub lastname: String,
    pub id_token_exp: i64,
    pub language: String,
    pub display_name: String,
}

pub struct NewUserWithGroups {
    pub new_user: NewUser,
    pub groups: Vec<String>,
}

impl NewUserWithGroups {
    /// Inserts a new user
    ///
    /// Returns the inserted users with the respective GroupIds
    #[tracing::instrument(err, skip_all)]
    pub fn insert(self, conn: &DbConnection) -> Result<(User, Vec<GroupId>)> {
        conn.transaction::<_, DatabaseError, _>(|| {
            let user: User = diesel::insert_into(users::table)
                .values(self.new_user)
                .get_result(conn)?;

            let groups = get_ids_for_group_names(conn, &user, self.groups)?;
            let group_ids = groups.iter().map(|(id, _)| *id).collect();
            insert_user_into_user_groups(conn, &user, groups)?;

            Ok((user, group_ids))
        })
    }
}

/// Diesel user struct for updates
///
/// Is used in update queries. None fields will be ignored on update queries
#[derive(Default, AsChangeset)]
#[table_name = "users"]
pub struct UpdateUser {
    pub title: Option<String>,
    pub display_name: Option<String>,
    pub language: Option<String>,
    pub id_token_exp: Option<i64>,
    pub dashboard_theme: Option<String>,
    pub conference_theme: Option<String>,
}

/// Ok type of [`UpdateUser::apply`]
pub struct UserUpdatedInfo {
    /// The user after the modification
    pub user: User,

    /// True the user's groups changed.
    /// Relevant for permission related state
    pub groups_changed: bool,

    pub groups_added: Vec<GroupId>,
    pub groups_removed: Vec<GroupId>,
}

impl UpdateUser {
    #[tracing::instrument(err, skip_all)]
    pub fn apply(
        self,
        conn: &DbConnection,
        user_id: UserId,
        groups: Option<Vec<String>>,
    ) -> Result<UserUpdatedInfo> {
        conn.transaction::<UserUpdatedInfo, DatabaseError, _>(move || {
            let query = diesel::update(users::table.filter(users::id.eq(user_id))).set(self);
            let user: User = query.get_result(conn)?;

            // modify groups if parameter exists
            if let Some(groups) = groups {
                let groups = get_ids_for_group_names(conn, &user, groups)?;

                let curr_groups = Group::get_all_for_user(conn, user.id)?;

                let added = groups
                    .iter()
                    .filter(|(_, name)| !curr_groups.iter().any(|curr_grp| curr_grp.name == *name))
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>();

                let removed = curr_groups
                    .iter()
                    .filter(|curr| !groups.iter().any(|(_, name)| name == &curr.name))
                    .map(|grp| grp.id)
                    .collect::<Vec<_>>();

                let groups_changed = !added.is_empty() || !removed.is_empty();

                if groups_changed {
                    // Remove user from user_groups table
                    let target = user_groups::table.filter(user_groups::user_id.eq(user.id));
                    diesel::delete(target).execute(conn).map_err(|e| {
                        log::error!("Failed to remove user's groups from user_groups, {}", e);
                        DatabaseError::from(e)
                    })?;

                    insert_user_into_user_groups(conn, &user, groups)?
                }

                Ok(UserUpdatedInfo {
                    user,
                    groups_changed,
                    groups_added: added,
                    groups_removed: removed,
                })
            } else {
                Ok(UserUpdatedInfo {
                    user,
                    groups_changed: false,
                    groups_added: vec![],
                    groups_removed: vec![],
                })
            }
        })
    }
}

// Returns ids of groups
// If the group is currently not stored, create a new group and returns the ID along the already present ones.
// Does not preserve the order of groups passed to the function
fn get_ids_for_group_names(
    conn: &DbConnection,
    user: &User,
    groups: Vec<String>,
) -> Result<Vec<(GroupId, String)>> {
    let present_groups: Vec<(GroupId, String)> = groups::table
        .select((groups::id, groups::name))
        .filter(
            groups::oidc_issuer
                .eq(user.oidc_issuer.as_ref())
                .and(groups::name.eq_any(&groups)),
        )
        .load(conn)?;

    let new_groups: Vec<NewGroup> = groups
        .into_iter()
        .filter(|wanted| !present_groups.iter().any(|(_, name)| name == wanted))
        .map(|name| NewGroup {
            oidc_issuer: user.oidc_issuer.clone(),
            name,
        })
        .collect();

    let new_groups: Vec<(GroupId, String)> = diesel::insert_into(groups::table)
        .values(&new_groups)
        .returning((groups::id, groups::name))
        .load(conn)?;

    Ok(present_groups
        .into_iter()
        .chain(new_groups.into_iter())
        .collect())
}

#[tracing::instrument(err, skip_all)]
fn insert_user_into_user_groups(
    conn: &DbConnection,
    user: &User,
    groups: Vec<(GroupId, String)>,
) -> Result<()> {
    let new_user_groups = groups
        .into_iter()
        .map(|(id, _)| NewUserGroupRelation {
            user_id: user.id,
            group_id: id,
        })
        .collect::<Vec<_>>();

    diesel::insert_into(user_groups::table)
        .values(new_user_groups)
        .on_conflict_do_nothing()
        .execute(conn)
        .map_err(|e| {
            log::error!("Failed to insert user_groups, {}", e);
            DatabaseError::from(e)
        })?;

    Ok(())
}
