//! Allows to manipulate the acls
//! Currently supported is enabling/disabling room access for all users.
use super::AclSubCommand;
use crate::acl::{check_or_create_kustos_role_policy, maybe_remove_kustos_role_policy};
use anyhow::{Context, Result};
use controller_shared::settings::Settings;
use database::Db;
use kustos::prelude::AccessMethod;
use std::sync::Arc;

pub(crate) async fn acl(settings: Settings, e: AclSubCommand) -> Result<()> {
    match e {
        AclSubCommand::UsersHaveAccessToAllRooms { action } => match action {
            super::EnableDisable::Enable => enable_user_access_to_all_rooms(&settings).await?,
            super::EnableDisable::Disable => disable_user_access_to_all_rooms(&settings).await?,
        },
    }
    Ok(())
}

async fn enable_user_access_to_all_rooms(settings: &Settings) -> Result<()> {
    let db = Arc::new(Db::connect(&settings.database).context("Failed to connect to database")?);
    let authz = kustos::Authz::new(db.clone()).await?;

    check_or_create_kustos_role_policy(
        &authz,
        "user",
        "/rooms/{roomUuid}/start",
        AccessMethod::POST,
    )
    .await?;
    check_or_create_kustos_role_policy(&authz, "user", "/rooms/{roomUuid}", AccessMethod::GET)
        .await?;
    println!("Enabled access for all users to all rooms");
    Ok(())
}

async fn disable_user_access_to_all_rooms(settings: &Settings) -> Result<()> {
    let db = Arc::new(Db::connect(&settings.database).context("Failed to connect to database")?);
    let authz = kustos::Authz::new(db.clone()).await?;

    maybe_remove_kustos_role_policy(
        &authz,
        "user",
        "/rooms/{roomUuid}/start",
        AccessMethod::POST,
    )
    .await?;
    maybe_remove_kustos_role_policy(&authz, "user", "/rooms/{roomUuid}", AccessMethod::GET).await?;
    println!("Disabled access for all users to all rooms");
    Ok(())
}
