//! Fixes acl rules based on the database content
//! Currently it can add users to roles and their groups.
//! Might fix invite acls and room access acl in the future too.
// TODO(r.floren) We might want to change these to batched fixed in the future,
// depending on the memory footprint
use anyhow::{Context, Error, Result};
use controller_shared::settings::Settings;
use database::Db;
use db_storage::{DbRoomsEx, DbUsersEx};
use kustos::{
    prelude::{AccessMethod, PolicyUser},
    Resource,
};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) struct FixAclConfig {
    pub(crate) user_roles: bool,
    pub(crate) user_groups: bool,
    pub(crate) room_creators: bool,
}

pub(crate) async fn fix_acl(settings: Settings, config: FixAclConfig) -> Result<()> {
    let db = Arc::new(Db::connect(&settings.database).context("Failed to connect to database")?);
    let authz = kustos::Authz::new(db.clone()).await?;

    // Used to collect errors during looped operations
    let mut errors: Vec<Error> = Vec::new();
    if config.user_groups || config.user_roles {
        fix_user(&config, db.clone(), &authz, &mut errors).await?;
    }
    if config.room_creators {
        fix_rooms(&config, db, &authz, &mut errors).await?;
    }

    if errors.is_empty() {
        println!("ACLs fixed");
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{}",
            errors
                .iter()
                .map(|e| format!("{:#} \n", e))
                .collect::<String>()
        ))
    }
}

async fn fix_user(
    config: &FixAclConfig,
    db: Arc<Db>,
    authz: &kustos::Authz,
    errors: &mut Vec<Error>,
) -> Result<()> {
    let users = db.get_users_with_groups().context("Failed to load users")?;
    for (user, groups) in users {
        if config.user_roles {
            let needs_addition = !match authz.is_user_in_role(user.oidc_uuid, "user").await {
                Ok(in_role) => in_role,
                Err(e) => {
                    errors.push(e.into());
                    false
                }
            };

            if needs_addition {
                match authz.add_user_to_role(user.oidc_uuid, "user").await {
                    Ok(_) => {}
                    Err(e) => errors.push(e.into()),
                }
            }
        }
        if config.user_groups {
            for group in groups {
                let needs_addition = !match authz
                    .is_user_in_group(user.oidc_uuid, group.id.clone())
                    .await
                    .with_context(|| format!("User: {}, Group: {}", user.oidc_uuid, group.id))
                {
                    Ok(in_group) => in_group,
                    Err(e) => {
                        errors.push(e);
                        false
                    }
                };

                if needs_addition {
                    match authz
                        .add_user_to_group(user.oidc_uuid, group.id.clone())
                        .await
                        .with_context(|| format!("User: {}, Group: {}", user.oidc_uuid, group.id))
                    {
                        Ok(_) => {}
                        Err(e) => errors.push(e),
                    }
                }
            }
        }
    }
    Ok(())
}

async fn fix_rooms(
    _config: &FixAclConfig,
    db: Arc<Db>,
    authz: &kustos::Authz,
    errors: &mut Vec<Error>,
) -> Result<()> {
    let rooms = db
        .get_rooms_with_creator()
        .context("failed to load rooms")?;
    for (room, user) in rooms {
        match maybe_grant_access_to_user(
            authz,
            user.oidc_uuid,
            room.uuid.resource_id(),
            &[AccessMethod::Get, AccessMethod::Put, AccessMethod::Delete],
        )
        .await
        {
            Ok(_) => {}
            Err(e) => errors.push(e),
        }
        match maybe_grant_access_to_user(
            authz,
            user.oidc_uuid,
            room.uuid.resource_id().with_suffix("/invites"),
            &[AccessMethod::Post, AccessMethod::Get],
        )
        .await
        {
            Ok(_) => {}
            Err(e) => errors.push(e),
        }
        match maybe_grant_access_to_user(
            authz,
            user.oidc_uuid,
            room.uuid.resource_id().with_suffix("/start"),
            &[AccessMethod::Post],
        )
        .await
        {
            Ok(_) => {}
            Err(e) => errors.push(e),
        }
    }
    Ok(())
}

async fn maybe_grant_access_to_user(
    authz: &kustos::Authz,
    user: Uuid,
    res: kustos::ResourceId,
    access: &[AccessMethod],
) -> Result<()> {
    let needs_addition = !authz
        .is_permissions_present(PolicyUser::from(user), res.clone(), access)
        .await
        .with_context(|| format!("User: {}, Resource: {:?}", user, res))?;
    if needs_addition {
        return authz
            .grant_user_access(user, &[(&res, access)])
            .await
            .with_context(|| format!("User: {}, Resource: {:?}", user, res));
    }
    Ok(())
}
