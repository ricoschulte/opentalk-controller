use anyhow::Result;
use controller::db::migrations::migrate_from_url;
use controller::db::rooms::{NewRoom, Room, RoomId};
use controller::db::users::{NewUser, NewUserWithGroups, User, UserId};
use controller::db::DbInterface;
use controller::prelude::anyhow::Context;
use controller::prelude::*;
use diesel::{Connection, PgConnection, RunQueryDsl};
use std::sync::Arc;
use uuid::Uuid;

/// Contains the [`DbInterface`] as well as information about the test database
pub struct DatabaseContext {
    pub base_url: String,
    pub db_name: String,
    pub db_conn: Arc<DbInterface>,
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

        let postgres_url = format!("{}/postgres", base_url);
        let conn =
            PgConnection::establish(&postgres_url).expect("Cannot connect to postgres database.");

        // Drop the target database in case it already exists to guarantee a clean state
        drop_database(&conn, &db_name).expect("Database initialization cleanup failed");

        // Create a new database for the test
        let query = diesel::sql_query(format!("CREATE DATABASE {}", db_name).as_str());
        query
            .execute(&conn)
            .expect(format!("Could not create database {}", db_name).as_str());

        let db_url = format!("{}/{}", base_url, db_name);

        migrate_from_url(&db_url)
            .await
            .expect("Unable to migrate database");

        let db_conn = Arc::new(DbInterface::connect_url(&db_url, 5, None).unwrap());

        Self {
            base_url: base_url.to_string(),
            db_name: db_name.to_string(),
            db_conn,
            drop_db_on_drop,
        }
    }

    pub fn create_test_user(&self, id: UserId) -> Result<User> {
        let user_uuid = Uuid::from_u128(id.into_inner() as u128);

        let new_user = NewUser {
            oidc_uuid: user_uuid,
            email: format!("k3k_test_user{}@heinlein.de", id),
            title: "".into(),
            firstname: "test".into(),
            lastname: "tester".into(),
            id_token_exp: 0,
            theme: "".into(),
            language: "en".into(),
        };

        let new_user_with_groups = NewUserWithGroups {
            new_user,
            groups: vec![],
        };

        self.db_conn.create_user(new_user_with_groups)?;

        Ok(self
            .db_conn
            .get_user_by_uuid(&user_uuid)
            .map(|user| user.expect("Expected user1 to exist"))?)
    }

    pub fn create_test_room(&self, room_id: RoomId, owner: UserId) -> Result<Room> {
        let new_room = NewRoom {
            uuid: room_id,
            owner,
            password: "".into(),
            wait_for_moderator: false,
            listen_only: false,
        };

        Ok(self.db_conn.new_room(new_room)?)
    }
}

impl Drop for DatabaseContext {
    fn drop(&mut self) {
        if self.drop_db_on_drop {
            let postgres_url = format!("{}/postgres", self.base_url);
            let conn = PgConnection::establish(&postgres_url)
                .expect("Cannot connect to postgres database.");

            drop_database(&conn, &self.db_name).unwrap();
        }
    }
}

/// Disconnect all users from the database with `db_name` and drop it.
fn drop_database(conn: &PgConnection, db_name: &str) -> Result<()> {
    let query = diesel::sql_query(format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", db_name));
    query
        .execute(conn)
        .with_context(|| format!("Couldn't drop database {}", db_name))?;

    Ok(())
}
