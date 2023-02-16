// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use chrono::{DateTime, Utc};
use jsonwebtoken::{self, decode, Algorithm, DecodingKey, Validation};
use openidconnect::core::{CoreJsonWebKeySet, CoreJwsSigningAlgorithm};
use openidconnect::JsonWebKey;
use serde::de::DeserializeOwned;
use std::time::SystemTime;

/// Token Verification errors
///
/// The error messages will get displayed in a HTTP 401 response
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VerifyError {
    #[error("Not a valid JWT ({0})")]
    InvalidJwt(String),
    #[error("JWT has invalid claims")]
    InvalidClaims,
    #[error("JWT token expired ({0})")]
    Expired(String),
    #[error("JWT header does not specify a KeyID (kid)")]
    MissingKeyID,
    #[error("JWT header specified an unknown KeyID (kid)")]
    UnknownKeyID,
    #[error("JWT has an invalid signature")]
    InvalidSignature,
}

/// Contains all claims that are needed to verify any JWT Token
pub trait VerifyClaims: DeserializeOwned {
    fn exp(&self) -> DateTime<Utc>;
}

/// Verify a raw JWT.
///
/// Returns `Err(_)` if the JWT is invalid or expired.
pub fn verify<C: VerifyClaims>(key_set: &CoreJsonWebKeySet, token: &str) -> Result<C, VerifyError> {
    let header = jsonwebtoken::decode_header(token).map_err(|e| {
        log::warn!("Unable to parse token header, {}", e);
        VerifyError::InvalidJwt("Unable to parse token header".to_string())
    })?;

    // Get the token key id
    let token_kid = match header.kid.as_ref() {
        None => {
            log::warn!("Missing key ID in token");
            return Err(VerifyError::MissingKeyID);
        }
        Some(token_kid) => token_kid,
    };

    // Find the public key using the key id
    let signing_key_opt = key_set.keys().iter().find(|key| {
        key.key_id()
            .map(|kid| kid.as_str() == token_kid)
            .unwrap_or_default()
    });

    let signing_key = match signing_key_opt {
        None => {
            log::warn!("Unknown key ID in token");
            return Err(VerifyError::UnknownKeyID);
        }
        Some(signing_key) => signing_key,
    };

    let (message, signature) = match token.rsplit_once('.') {
        None => {
            let msg = "Unable to parse signature from JWT token";
            log::warn!("{}", msg);
            return Err(VerifyError::InvalidJwt(msg.to_string()));
        }
        Some((message, signature)) => (message, signature),
    };

    // Decode the JWT signature
    let signature = base64::decode_config(signature, base64::URL_SAFE_NO_PAD).map_err({
        |e| {
            log::warn!("Token has invalid signature, {}", e);
            VerifyError::InvalidSignature
        }
    })?;

    // Verify the signature using the openidconnect crate
    let signing_alg = map_algorithm(header.alg).ok_or(VerifyError::InvalidSignature)?;

    signing_key
        .verify_signature(&signing_alg, message.as_ref(), signature.as_ref())
        .map_err(|_| VerifyError::InvalidSignature)?;

    // We can ignore the signature check of jsonwebtokens
    // TODO use jsonwebtokens validation
    let mut validation = Validation::default();
    validation.insecure_disable_signature_validation();
    // This is done manually at the end currently.
    validation.validate_exp = false;

    // Just parse the token out, no verification
    let token = decode::<C>(token, &DecodingKey::from_secret(&[]), &validation).map_err(|e| {
        log::warn!("Unable to decode claims from provided id token, {}", e);
        VerifyError::InvalidClaims
    })?;

    let now = DateTime::<Utc>::from(SystemTime::now());

    // Verify expiration
    if now > token.claims.exp() {
        let msg = format!("The provided token expired at {}", token.claims.exp());
        log::warn!("{}", msg);
        return Err(VerifyError::Expired(msg));
    }

    Ok(token.claims)
}

/// Map jsonwebtoken `Algorithm` to `CoreJwsSigningAlgorithm`
fn map_algorithm(alg: Algorithm) -> Option<CoreJwsSigningAlgorithm> {
    match alg {
        Algorithm::HS256 => Some(CoreJwsSigningAlgorithm::HmacSha256),
        Algorithm::HS384 => Some(CoreJwsSigningAlgorithm::HmacSha384),
        Algorithm::HS512 => Some(CoreJwsSigningAlgorithm::HmacSha512),
        Algorithm::ES256 => Some(CoreJwsSigningAlgorithm::EcdsaP256Sha256),
        Algorithm::ES384 => Some(CoreJwsSigningAlgorithm::EcdsaP384Sha384),
        Algorithm::RS256 => Some(CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256),
        Algorithm::RS384 => Some(CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384),
        Algorithm::RS512 => Some(CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512),
        Algorithm::PS256 => Some(CoreJwsSigningAlgorithm::RsaSsaPssSha256),
        Algorithm::PS384 => Some(CoreJwsSigningAlgorithm::RsaSsaPssSha384),
        Algorithm::PS512 => Some(CoreJwsSigningAlgorithm::RsaSsaPssSha512),
        Algorithm::EdDSA => None,
    }
}

#[cfg(test)]
mod test {
    use crate::oidc::UserClaims;

    use super::VerifyError;
    use openidconnect::core::{CoreJsonWebKey, CoreJsonWebKeySet, CoreJwsSigningAlgorithm};
    use openidconnect::{JsonWebKey, JsonWebKeyId};
    use pretty_assertions::assert_eq;
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
            format!(r#"{{ "alg": "RS256", "typ": "JWT", "kid": "{KID}" }}"#),
            base64::URL_SAFE_NO_PAD,
        );

        let payload = format!(
            r#"{{
                "sub": "admin",
                "iat": {iat},
                "iss": "https://example.org/realms/Test",
                "exp": {exp},
                "x_grp": ["/admin"],
                "preferred_username": "admin",
                "given_name": "the",
                "family_name": "admin",
                "email": "admin@mail.de",
                "tenant_id": "customer-123"
                }}"#,
        );

        let payload = base64::encode_config(payload, base64::URL_SAFE_NO_PAD);

        format!("{header}.{payload}")
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
        let jwt_enc = format!("{message}.{signature}");

        // Verify should success
        let claims =
            super::verify::<UserClaims>(&jwks, &jwt_enc).expect("Valid JWT failed to verify");

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
        let jwt_enc = format!("{message}.{signature}");

        // Verify should success
        match super::verify::<UserClaims>(&jwks, &jwt_enc) {
            Ok(_) => panic!("Test must fail, exp is set in the past"),
            Err(e) => {
                if let VerifyError::Expired(_) = e {
                    // Test successful
                } else {
                    panic!("Test must fail with VerifyError::Expired(...)")
                }
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
        let jwt_enc = format!("{message}.{signature}");

        // Verify should success
        match super::verify::<UserClaims>(&jwks, &jwt_enc) {
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
