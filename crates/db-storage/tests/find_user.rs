use database::Db;
use k3k_db_storage::users::{DbUsersEx, NewUser, NewUserWithGroups};
use serial_test::serial;

fn make_user(db: &Db, firstname: &str, lastname: &str) {
    db.create_user(NewUserWithGroups {
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
            theme: "".into(),
            language: "".into(),
            oidc_sub: format!("{}{}", firstname, lastname),
            oidc_issuer: "".into(),
        },
        groups: vec![],
    })
    .unwrap();
}

#[tokio::test]
#[serial]
async fn test() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;
    let db = &db_ctx.db_conn;

    // generate some random users with some made up names
    make_user(db, "Aileen", "Strange");
    make_user(db, "Laura", "Rutherford");
    make_user(db, "Cheryl", "Lazarus");

    let users = db.find_users_by_name("La").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Cheryl");
    assert_eq!(users[1].firstname, "Laura");

    let users = db.find_users_by_name("Ru").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Cheryl");
    assert_eq!(users[1].firstname, "Laura");

    // Try the levenshtein/soundex matching with worse input each time
    let users = db.find_users_by_name("Cheril Lazarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = db.find_users_by_name("Cheril Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = db.find_users_by_name("Cherill Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = db.find_users_by_name("Cherill Lasaruz").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");
}
