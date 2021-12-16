use anyhow::{Context, Result};
use kustos::prelude::*;

/// Checks whether the default permissions are present. These are the mandatory permissions.
///
/// If you introduce new endpoints that need user post access, add these permissions here.
pub(crate) async fn check_or_create_kustos_default_permissions(authz: &Authz) -> Result<()> {
    if !authz
        .is_group_in_role("/OpenTalk_Administrator", "administrator")
        .await?
    {
        authz
            .add_group_to_role("/OpenTalk_Administrator", "administrator")
            .await
            .context("Adding default administrator role to /OpenTalk_Administrator")?;
    }
    check_or_create_kustos_role_policy(authz, "administrator", "/*", AccessMethod::all_http())
        .await?;
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
        [AccessMethod::Put, AccessMethod::Get],
    )
    .await?;

    Ok(())
}

async fn check_or_create_kustos_role_policy<P, R, A>(
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
