use crate::common::make_user;
use k3k_db_storage::users::User;
use serial_test::serial;

mod common;

#[tokio::test]
#[serial]
async fn test() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;
    let mut conn = db_ctx.db.get_conn().unwrap();

    // generate some random users with some made up names
    make_user(&mut conn, "Aileen", "Strange", "Spectre");
    make_user(&mut conn, "Laura", "Rutherford", "Jakiro");
    make_user(&mut conn, "Cheryl", "Lazarus", "Kaolin");

    let users = User::find(&mut conn, "La").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Cheryl");
    assert_eq!(users[1].firstname, "Laura");

    let users = User::find(&mut conn, "Ru").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Laura");
    assert_eq!(users[1].firstname, "Cheryl");

    // Try the levenshtein/soundex matching with worse input each time
    let users = User::find(&mut conn, "Cheril Lazarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, "Cheril Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, "Cherill Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, "Cherill Lasaruz").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, "Spectre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");

    let users = User::find(&mut conn, "Spektre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");

    let users = User::find(&mut conn, "Schpecktre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");
}
