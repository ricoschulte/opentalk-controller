use chrono::{DateTime, Utc};
use jsonwebtoken::{dangerous_insecure_decode, Algorithm};
use openidconnect::core::{CoreJsonWebKeySet, CoreJwsSigningAlgorithm};
use openidconnect::JsonWebKey;
use serde::Deserialize;
use std::time::SystemTime;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VerifyError {
    #[error("Not a JWT")]
    InvalidJwt,
    #[error("JWT is expired")]
    Expired,
    #[error("JWT header does not specify a KeyID (kid)")]
    MissingKeyID,
    #[error("JWT header specified an unknown KeyID (kid)")]
    UnknownKeyID,
    #[error("JWT has an invalid signature")]
    InvalidSignature,
}

/// Contains all claims that are needed to verify any JWT Token
#[derive(Deserialize)]
pub struct VerifyClaims {
    /// Expires at
    #[serde(with = "time")]
    pub exp: DateTime<Utc>,
    /// Issued at
    #[serde(with = "time")]
    pub iat: DateTime<Utc>,
    /// Subject (User ID)
    pub sub: String,
    /// Groups the user belongs to.  
    /// This is a custom field not specified by the OIDC Standard
    pub x_grp: Vec<String>,
}

/// Verify a raw JWT.
///
/// Returns `Err(_)` if the JWT is invalid or expired.
pub fn verify(key_set: &CoreJsonWebKeySet, token: &str) -> Result<VerifyClaims, VerifyError> {
    let (message, signature) = token.rsplit_once('.').ok_or(VerifyError::InvalidJwt)?;

    // Just parse the token out, no verification
    let token =
        dangerous_insecure_decode::<VerifyClaims>(token).map_err(|_| VerifyError::InvalidJwt)?;

    let now = DateTime::<Utc>::from(SystemTime::now());

    // Verify if expired
    if now > token.claims.exp {
        return Err(VerifyError::Expired);
    }

    // Get the token key id
    let token_kid = token.header.kid.as_ref().ok_or(VerifyError::MissingKeyID)?;

    // Find the public key using the key id
    let signing_key = key_set
        .keys()
        .iter()
        .find(|key| {
            key.key_id()
                .map(|kid| kid.as_str() == token_kid)
                .unwrap_or_default()
        })
        .ok_or(VerifyError::UnknownKeyID)?;

    // Decode the JWT signature
    let signature = base64::decode_config(signature, base64::URL_SAFE_NO_PAD)
        .map_err(|_| VerifyError::InvalidSignature)?;

    // Verify the signature
    let signing_alg = map_algorithm(token.header.alg);

    let () = signing_key
        .verify_signature(&signing_alg, message.as_ref(), signature.as_ref())
        .map_err(|_| VerifyError::InvalidSignature)?;

    Ok(token.claims)
}

/// Map jsonwebtoken `Algorithm` to `CoreJwsSigningAlgorithm`
fn map_algorithm(alg: Algorithm) -> CoreJwsSigningAlgorithm {
    match alg {
        Algorithm::HS256 => CoreJwsSigningAlgorithm::HmacSha256,
        Algorithm::HS384 => CoreJwsSigningAlgorithm::HmacSha384,
        Algorithm::HS512 => CoreJwsSigningAlgorithm::HmacSha512,
        Algorithm::ES256 => CoreJwsSigningAlgorithm::EcdsaP256Sha256,
        Algorithm::ES384 => CoreJwsSigningAlgorithm::EcdsaP384Sha384,
        Algorithm::RS256 => CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
        Algorithm::RS384 => CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384,
        Algorithm::RS512 => CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512,
        Algorithm::PS256 => CoreJwsSigningAlgorithm::RsaSsaPssSha256,
        Algorithm::PS384 => CoreJwsSigningAlgorithm::RsaSsaPssSha384,
        Algorithm::PS512 => CoreJwsSigningAlgorithm::RsaSsaPssSha512,
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

#[cfg(test)]
mod test {
    use super::VerifyError;
    use openidconnect::core::{CoreJsonWebKey, CoreJsonWebKeySet, CoreJwsSigningAlgorithm};
    use openidconnect::{JsonWebKey, JsonWebKeyId};
    use ring::rand::SystemRandom;
    use ring::signature::{KeyPair, RsaKeyPair, RSA_PKCS1_SHA256};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Test PKCS8 Key Pair to sign and validate data
    static PKCS8: &str = "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCq3q7ElUZrf6Kih4YLY3PDMLyItdG2kk3HqzLCYFX3Xe6Mqxrq4AWnr3Bu8V6TWciV20jZupxxbE/wMAqmh+VXXEAQmT8OfgWOblQfvaY3mSR7hMCJ6bqkm1f2TyCEDia+2NyIWbyBUI1YLGFwcNPgY49G49Xs6YrwhmQTXx+yN/YQ0rHelIy3hN/MLD10QSPMVAKOApywa2sXr06tsgw8R/57d8OrM6DcbIYZli5CWRGoSHGw4k02ytvzp9CuxPVgC0+AHftDSFWdFh73R3vkr6SPyAWLZNh56xEfyntLp1WIAs3mh8zMG4jaujunsw7EAPMGpdros9a3YtsyePddAgMBAAECggEAEAjRkbUIZLIXivT4yTzN8jUynAmj4mQcVG5mVwM/TfVMm3q7Detz3GaEQIT6AQ3d2uI3FeeDIsmtPrbjaPk7tlT71hLrbeq5jsIfttLPNEx0tfqhLs/2KdhCCuUmAf5p+GLVXx48qE3s1adkhW6xE0+EdHyQ6KiJ10RlQ8Qbb1fVpN2mS4NAqFALRWR6qoJTLQca/yZ65U/FOn30i6adPnrCFeFyaz4MuGA2KwH97BRsE58Eh7qqq6gvNuFhsmAyc80mgvR656AzsS2iBNQ1sz9F00yKXrJ511eMAbzaxkYbtIvfh/0lRP8d0fmke6spGRL7rRk0aUJmzy2trmL86QKBgQDjjUCQFXtW+eUZSVtPXTFPGbTHnmC5eGznol9ih4IRv4PMwua6MH518M3kh5ALDZOKvr6+jtoYD0olQ/IIvMvRPYpp7RyJbpmfHI9ASk2hR9DuntgDNgOm9yygLRY3h8/ZFh/1Gm7Gk+11U6Z/eOQMLIidaNe2MsEZjfnN7Lb3UwKBgQDAO1ZkVrqg53h/b+2mSSNofz5oNur7HuhYkpFQu+jYDwpE16OW+qWEkKtt50R0X7ceLezuC1ZyqgGifruq+80mTKVDz6e0bZIE84anSzqEzXLazEqZ4kpG1LB7YmR86YSmtau7FP+UfcM63EQrL70+2zvcuygTdU+P49/mZB3wjwKBgQCYF6V7qKAT9ltml11sooF+uVPXyMglr5Q7DpBqruAFNNjHV84XzKn58sXrZaClgqGHLw8XFyw2wKFyXwO7S1V/uX52ZoGYalBLxS8KbZ+NmQ7RL2J6YvP1+Wfed8RNwXzvQJaDoPNBz0X8EblLomXqrSly7MyhfzMJ/ZdmSD3S+QKBgAIdZQDrl1gH0+KLB7FJorMWm0goOoOSvnmi+yhJOPGPkMxbFvilP0brFIe8AJvLJceWN8ISq9vNFQGFpWjnJkWimDrbwPuSLQYS68tRX45weDACCVwSCkEnO93Pok1hgE0ZOI9xVrJ6g7hVDgbvmoRjgxAVmwZDxyFNH3x4Y3/vAoGAAqFGQUR3ouLMdug8grBuLBLa8L7BN+DTDsSlKaheAPvcuhPuPX7DP4cXR3aMoZii3V/dqzJiM0Hg7ERA+TcwMhXqKIjMOsUfVioVFT315k4FaZ/aR5B+6VC07FEiw2/kiy702g1ArY0kRypK1dx84TXIzdG8mtu8jx6iFr/x/u4=";
    static KID: &str = "MyKeyID";

    fn setup_keys() -> (RsaKeyPair, CoreJsonWebKeySet) {
        let key = base64::decode(PKCS8).unwrap();
        let key_pair = RsaKeyPair::from_pkcs8(&key).unwrap();

        let jwk = {
            let public = key_pair.public_key();
            let n = public.modulus().big_endian_without_leading_zero().to_vec();
            let e = public.exponent().big_endian_without_leading_zero().to_vec();

            CoreJsonWebKey::new_rsa(n, e, Some(JsonWebKeyId::new(KID.into())))
        };

        let jwks = CoreJsonWebKeySet::new(vec![jwk]);

        (key_pair, jwks)
    }

    fn sign_data(pair: &RsaKeyPair, data: &[u8]) -> [u8; 256] {
        let rng = SystemRandom::new();
        let mut signature = [0; 256];

        pair.sign(&RSA_PKCS1_SHA256, &rng, data.as_ref(), signature.as_mut())
            .unwrap();

        signature
    }

    fn build_jwt_message(iat: u64, exp: u64) -> String {
        let header = base64::encode_config(
            format!(r#"{{ "alg": "RS256", "typ": "JWT", "kid": "{}" }}"#, KID),
            base64::URL_SAFE_NO_PAD,
        );

        let payload = format!(
            r#"{{
                "sub": "admin",
                "iat": {},
                "exp": {},
                "x_grp": ["/admin"]
                }}"#,
            iat, exp,
        );

        let payload = base64::encode_config(payload, base64::URL_SAFE_NO_PAD);

        format!("{}.{}", header, payload)
    }

    #[test]
    fn basic_sign_validation() {
        let (pair, jwks) = setup_keys();

        let message = "Some data!";

        let signature = sign_data(&pair, message.as_ref());

        jwks.keys()[0]
            .verify_signature(
                &CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
                message.as_ref(),
                signature.as_ref(),
            )
            .unwrap();
    }

    #[test]
    fn valid_jwt() {
        let secs_since_unix_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let (pair, jwks) = setup_keys();

        let message = build_jwt_message(
            // Set the issued at time to 100 secs in the past
            secs_since_unix_epoch - 100,
            // and the expires time at 200 secs in the future
            secs_since_unix_epoch + 200,
        );

        // Sign with our private key
        let signature = sign_data(&pair, message.as_ref());
        let signature = base64::encode_config(signature.as_ref(), base64::URL_SAFE_NO_PAD);

        // complete JWT
        let jwt_enc = format!("{}.{}", message, signature);

        // Verify should success
        let claims = super::verify(&jwks, &jwt_enc).expect("Valid JWT failed to verify");

        assert_eq!(claims.sub, "admin");
        assert_eq!(claims.x_grp[0], "/admin");
    }

    #[test]
    fn expired_jwt() {
        let secs_since_unix_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let (pair, jwks) = setup_keys();

        let message = build_jwt_message(
            // Set both the issued at time and expires time into the past
            secs_since_unix_epoch - 400,
            secs_since_unix_epoch - 100,
        );

        // Sign with our private key
        let signature = sign_data(&pair, message.as_ref());
        let signature = base64::encode_config(signature.as_ref(), base64::URL_SAFE_NO_PAD);

        // complete JWT
        let jwt_enc = format!("{}.{}", message, signature);

        // Verify should success
        match super::verify(&jwks, &jwt_enc) {
            Ok(_) => panic!("Test must fail, exp is set in the past"),
            Err(e) => {
                assert_eq!(
                    e,
                    VerifyError::Expired,
                    "Test must fail with VerifyError::Expired"
                );
            }
        }
    }

    #[test]
    fn bad_signature() {
        let secs_since_unix_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let (_, jwks) = setup_keys();

        let message = build_jwt_message(secs_since_unix_epoch - 100, secs_since_unix_epoch + 300);

        // Create a signature which is complete garbage
        let signature = b"Some garbage signature";
        let signature = base64::encode_config(signature.as_ref(), base64::URL_SAFE_NO_PAD);

        // complete JWT
        let jwt_enc = format!("{}.{}", message, signature);

        // Verify should success
        match super::verify(&jwks, &jwt_enc) {
            Ok(_) => panic!("Test must fail, provided bad signature"),
            Err(e) => {
                assert_eq!(
                    e,
                    VerifyError::InvalidSignature,
                    "Test must fail with VerifyError::InvalidSignature"
                );
            }
        }
    }
}
