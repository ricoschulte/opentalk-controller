// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::common::make_user;
use k3k_db_storage::users::User;
use pretty_assertions::assert_eq;
use serial_test::serial;

mod common;

#[tokio::test]
#[serial]
async fn test() {
    let db_ctx = test_util::database::DatabaseContext::new(true).await;
    let mut conn = db_ctx.db.get_conn().unwrap();

    // generate some random users with some made up names
    let tenant_id = make_user(&mut conn, "Aileen", "Strange", "Spectre").tenant_id;
    make_user(&mut conn, "Laura", "Rutherford", "Jakiro");
    make_user(&mut conn, "Cheryl", "Lazarus", "Kaolin");

    let users = User::find(&mut conn, tenant_id, "La").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Cheryl");
    assert_eq!(users[1].firstname, "Laura");

    let users = User::find(&mut conn, tenant_id, "Ru").unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].firstname, "Laura");
    assert_eq!(users[1].firstname, "Cheryl");

    // Try the levenshtein/soundex matching with worse input each time
    let users = User::find(&mut conn, tenant_id, "Cheril Lazarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, tenant_id, "Cheril Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, tenant_id, "Cherill Lasarus").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, tenant_id, "Cherill Lasaruz").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Cheryl");

    let users = User::find(&mut conn, tenant_id, "Spectre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");

    let users = User::find(&mut conn, tenant_id, "Spektre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");

    let users = User::find(&mut conn, tenant_id, "Schpecktre").unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].firstname, "Aileen");
}
