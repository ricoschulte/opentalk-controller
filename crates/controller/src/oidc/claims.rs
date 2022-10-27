use super::jwt;
use chrono::{DateTime, Utc};
use email_address::EmailAddress;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ServiceClaims {
    /// Expires at
    #[serde(with = "time")]
    pub exp: DateTime<Utc>,
    /// Issued at
    #[serde(with = "time")]
    pub iat: DateTime<Utc>,
    /// Issuer (URL to the OIDC Provider)
    pub iss: String,
    /// Keycloak realm management
    pub realm_access: RealmAccess,
}

impl jwt::VerifyClaims for ServiceClaims {
    fn exp(&self) -> DateTime<Utc> {
        self.exp
    }
}

/// Keycloak realm-management claim which includes the realm specific roles of the client
/// Only included in
#[derive(Deserialize)]
pub struct RealmAccess {
    pub roles: Vec<String>,
}

/// Claims provided for a logged-in user
#[derive(Deserialize)]
pub struct UserClaims {
    /// Expires at
    #[serde(with = "time")]
    pub exp: DateTime<Utc>,
    /// Issued at
    #[serde(with = "time")]
    pub iat: DateTime<Utc>,
    /// Issuer (URL to the OIDC Provider)
    pub iss: String,
    /// Subject (User ID)
    pub sub: String,
    /// The users email
    pub email: EmailAddress,
    /// The users firstname
    pub given_name: String,
    /// The users lastname
    pub family_name: String,
    /// Groups the user belongs to.
    /// This is a custom field not specified by the OIDC Standard
    pub x_grp: Vec<String>,
    /// The users phone number, if configured
    pub phone_number: Option<String>,
    /// The users optional nickname
    pub nickname: Option<String>,
}

impl jwt::VerifyClaims for UserClaims {
    fn exp(&self) -> DateTime<Utc> {
        self.exp
    }
}

mod time {
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seconds: i64 = Deserialize::deserialize(deserializer)?;

        Utc.timestamp_opt(seconds, 0).single().ok_or_else(|| {
            serde::de::Error::custom(format!(
                "Failed to convert {} seconds to DateTime<Utc>",
                seconds
            ))
        })
    }
}
