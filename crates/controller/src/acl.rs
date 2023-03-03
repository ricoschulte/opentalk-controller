// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use anyhow::{Context, Result};
use kustos::prelude::*;

/// Checks whether the default permissions are present. These are the mandatory permissions.
///
/// If you introduce new endpoints that need user post access, add these permissions here.
pub(crate) async fn check_or_create_kustos_default_permissions(authz: &Authz) -> Result<()> {
    check_or_create_kustos_role_policy(
        authz,
        "user",
        "/rooms",
        [AccessMethod::Post, AccessMethod::Get],
    )
    .await?;
    check_or_create_kustos_role_policy(
        authz,
        "user",
        "/users/me",
        [AccessMethod::Patch, AccessMethod::Get],
    )
    .await?;
    check_or_create_kustos_role_policy(authz, "user", "/users/me/tariff", [AccessMethod::Get])
        .await?;
    check_or_create_kustos_role_policy(authz, "user", "/users/find", [AccessMethod::Get]).await?;
    check_or_create_kustos_role_policy(
        authz,
        "user",
        "/users/me/pending_invites",
        [AccessMethod::Get],
    )
    .await?;
    check_or_create_kustos_role_policy(
        authz,
        "user",
        "/events",
        [AccessMethod::Post, AccessMethod::Get],
    )
    .await?;

    Ok(())
}

pub(crate) async fn check_or_create_kustos_role_policy<P, R, A>(
    authz: &kustos::Authz,
    role: P,
    res: R,
    access: A,
) -> Result<()>
where
    P: Into<PolicyRole>,
    R: Into<ResourceId>,
    A: Into<Vec<AccessMethod>>,
{
    let role = role.into();
    let res = res.into();
    let access = access.into();

    if !authz
        .is_permissions_present(role.clone(), res.clone(), access.clone())
        .await?
    {
        authz
            .grant_role_access(role.clone(), &[(&res, access.as_ref())])
            .await
            .with_context(|| {
                format!(
                    "granting role {:?} {} access to {:?}",
                    role,
                    access
                        .iter()
                        .map(|s| s.as_ref())
                        .collect::<Vec<_>>()
                        .join(", "),
                    res
                )
            })?
    }
    Ok(())
}

pub(crate) async fn maybe_remove_kustos_role_policy<P, R, A>(
    authz: &kustos::Authz,
    role: P,
    res: R,
    access: A,
) -> Result<()>
where
    P: Into<PolicyRole>,
    R: Into<ResourceId>,
    A: Into<Vec<AccessMethod>>,
{
    let role = role.into();
    let res = res.into();
    let access = access.into();

    if authz
        .is_permissions_present(role.clone(), res.clone(), access.clone())
        .await?
    {
        authz
            .remove_role_permission(role.clone(), res.clone(), access.clone())
            .await
            .with_context(|| {
                format!(
                    "removing role {:?} {} access to {:?}",
                    role,
                    access
                        .iter()
                        .map(|s| s.as_ref())
                        .collect::<Vec<_>>()
                        .join(", "),
                    res
                )
            })?;
    }
    Ok(())
}
