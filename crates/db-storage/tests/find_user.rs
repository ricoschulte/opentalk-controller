use database::DbConnection;
use k3k_db_storage::users::{NewUser, NewUserWithGroups, User};
use serial_test::serial;

fn make_user(conn: &DbConnection, firstname: &str, lastname: &str, display_name: &str) {
    NewUserWithGroups {
        new_user: NewUser {
            email: format!(
                "{}.{}@example.org",
                firstname.to_lowercase(),
                lastname.to_lowercase()
            ),
            title: "".into(),
            firstname: firstname.into(),
            lastname: lastname.into(),
            id_token_exp: 0,
            display_name: display_name.into(),
            language: "".into(),
            oidc_sub: format!("{}{}", firstname, lastname),
            oidc_issuer: "".into(),
        },
        groups: vec![],
    }
    .insert(conn)
    .unwrap();
}

#[tokio::test]
#[serial]
async fn test() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;
    let conn = db_ctx.db.get_conn().unwrap();

    // generate some random users with some made up names
    make_user(&conn, "Aileen", "Strange", "Spectre");
    make_user(&conn, "Laura", "Rutherford", "Jakiro");
    make_user(&conn, "Cheryl", "Lazarus", "Kaolin");

    let users = User::find(&conn, "La").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Cheryl");
    assert_eq!(users[1].firstname, "Laura");

    let users = User::find(&conn, "Ru").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Laura");
    assert_eq!(users[1].firstname, "Cheryl");

    // Try the levenshtein/soundex matching with worse input each time
    let users = User::find(&conn, "Cheril Lazarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&conn, "Cheril Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&conn, "Cherill Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&conn, "Cherill Lasaruz").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&conn, "Spectre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");

    let users = User::find(&conn, "Spektre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");

    let users = User::find(&conn, "Schpecktre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");
}
