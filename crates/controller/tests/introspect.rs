use anyhow::Result;
use k3k_controller_client::api;
use serial_test::serial;

mod common;

#[tokio::test]
#[serial]
#[ignore]
async fn introspect() -> Result<()> {
    common::setup_logging()?;

    // database will clean up when this gets dropped
    let _db_ctx = test_util::database::DatabaseContext::new(true).await;

    let mut controller = common::run_controller().await?;
    let session = common::setup_client("test", "test").await?;

    // Test inactive tokens
    let access_token_request = api::introspect::IntrospectRequest {
        token: session.tokens.access_token.secret().to_owned(),
        token_type_hint: None,
    };

    let inactive_response = api::introspect::check_introspect(
        &session.config.k3k_url,
        &session.http_client,
        &access_token_request,
    )
    .await?;
    assert!(!inactive_response.active);
    assert!(inactive_response.sub.is_none());

    // Test active tokens
    let _permissions = session.login().await?;

    let active_response = api::introspect::check_introspect(
        &session.config.k3k_url,
        &session.http_client,
        &access_token_request,
    )
    .await?;
    assert!(active_response.active);
    assert!(active_response.sub.is_some());

    controller.kill().await?;
    Ok(())
}
