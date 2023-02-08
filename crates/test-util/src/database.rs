// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::{Context, Result};
use database::Db;
use db_storage::groups::GroupName;
use db_storage::migrations::migrate_from_url;
use db_storage::rooms::{NewRoom, Room, RoomId};
use db_storage::users::{NewUser, NewUserWithGroups, User, UserId};
use diesel::{Connection, PgConnection, RunQueryDsl};
use std::sync::Arc;

/// Contains the [`Db`] as well as information about the test database
pub struct DatabaseContext {
    pub base_url: String,
    pub db_name: String,
    pub db: Arc<Db>,
    /// DatabaseContext will DROP the database inside postgres when dropped
    pub drop_db_on_drop: bool,
}

impl DatabaseContext {
    /// Create a new [`DatabaseContext`]
    ///
    /// Uses the environment variable `POSTGRES_BASE_URL` to connect to postgres. Defaults to `postgres://postgres:password123@localhost:5432`
    /// when the environment variable is not set. The same goes for `DATABASE_NAME` where the default is `k3k_test`.
    ///
    /// Once connected, the database with `DATABASE_NAME` gets dropped and re-created to guarantee a clean state, then the
    /// k3k controller migration is applied.
    pub async fn new(drop_db_on_drop: bool) -> Self {
        let base_url = std::env::var("POSTGRES_BASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:password123@localhost:5432".to_owned());

        let db_name = std::env::var("DATABASE_NAME").unwrap_or_else(|_| "k3k_test".to_owned());

        let postgres_url = format!("{base_url}/postgres");
        let mut conn =
            PgConnection::establish(&postgres_url).expect("Cannot connect to postgres database.");

        // Drop the target database in case it already exists to guarantee a clean state
        drop_database(&mut conn, &db_name).expect("Database initialization cleanup failed");

        // Create a new database for the test
        let query = diesel::sql_query(format!("CREATE DATABASE {db_name}"));
        query
            .execute(&mut conn)
            .unwrap_or_else(|_| panic!("Could not create database {db_name}"));

        let db_url = format!("{base_url}/{db_name}");

        migrate_from_url(&db_url)
            .await
            .expect("Unable to migrate database");

        let db_conn = Arc::new(Db::connect_url(&db_url, 5, None).unwrap());

        Self {
            base_url: base_url.to_string(),
            db_name: db_name.to_string(),
            db: db_conn,
            drop_db_on_drop,
        }
    }

    pub fn create_test_user(&self, n: u32, groups: Vec<String>) -> Result<User> {
        let new_user = NewUser {
            oidc_sub: format!("oidc_sub{n}"),
            email: format!("k3k_test_user{n}@heinlein.de"),
            title: "".into(),
            firstname: "test".into(),
            lastname: "tester".into(),
            id_token_exp: 0,
            display_name: "test tester".into(),
            language: "en".into(),
            phone: None,
        };

        let new_user_with_groups = NewUserWithGroups {
            new_user,
            groups: groups.into_iter().map(GroupName::from).collect(),
        };
        let mut conn = self.db.get_conn()?;
        let user = new_user_with_groups.insert(&mut conn)?;

        Ok(user.0)
    }

    pub fn create_test_room(
        &self,
        _room_id: RoomId,
        created_by: UserId,
        waiting_room: bool,
    ) -> Result<Room> {
        let new_room = NewRoom {
            created_by,
            password: None,
            waiting_room,
        };

        let mut conn = self.db.get_conn()?;

        let room = new_room.insert(&mut conn)?;

        Ok(room)
    }
}

impl Drop for DatabaseContext {
    fn drop(&mut self) {
        if self.drop_db_on_drop {
            let postgres_url = format!("{}/postgres", self.base_url);
            let mut conn = PgConnection::establish(&postgres_url)
                .expect("Cannot connect to postgres database.");

            drop_database(&mut conn, &self.db_name).unwrap();
        }
    }
}

/// Disconnect all users from the database with `db_name` and drop it.
fn drop_database(conn: &mut PgConnection, db_name: &str) -> Result<()> {
    let query = diesel::sql_query(format!("DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"));
    query
        .execute(conn)
        .with_context(|| format!("Couldn't drop database {db_name}"))?;

    Ok(())
}
