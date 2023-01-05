use anyhow::{Context, Error};
use basen::BASE58;
use redis_args::{FromRedisValue, ToRedisArgs};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, ToRedisArgs, FromRedisValue,
)]
#[to_redis_args(serde)]
#[from_redis_value(serde)]
pub struct Token(u64);

impl Token {
    pub fn new(v: u64) -> Self {
        Token(v)
    }
}

impl ToString for Token {
    fn to_string(&self) -> String {
        BASE58.encode_const_len(&self.0)
    }
}

impl FromStr for Token {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let v = BASE58
            .decode_const_len(s)
            .context(format!("not a base58-encoded u64 value: {:?}", s))?;
        Ok(Self(v))
    }
}

struct TokenVisitor;

impl<'de> de::Visitor<'de> for TokenVisitor {
    type Value = Token;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a base58-encoded u64 value")
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Token::from_str(s).map_err(|e| E::custom(e.to_string()))
    }
}

impl Serialize for Token {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Token {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(TokenVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_str() {
        assert_eq!(
            Token::from_str("1111Cn8eVZg").unwrap(),
            Token::new(0x68656c6c6f)
        );
    }

    #[test]
    fn serialize() {
        let t = Token::new(0x0);
        assert_eq!(serde_json::to_value(t).unwrap(), json!("11111111111"));

        let t = Token::new(0x30);
        assert_eq!(serde_json::to_value(t).unwrap(), json!("1111111111q"));

        let t = Token::new(0x68656c6c6f);
        assert_eq!(serde_json::to_value(t).unwrap(), json!("1111Cn8eVZg"));
    }

    #[test]
    fn deserialize() {
        let t: Token = serde_json::from_value(json!("11111111111")).unwrap();
        assert_eq!(t, Token::new(0));

        let t: Token = serde_json::from_value(json!("1111111111q")).unwrap();
        assert_eq!(t, Token::new(0x30));

        let t: Token = serde_json::from_value(json!("1111Cn8eVZg")).unwrap();
        assert_eq!(t, Token::new(0x68656c6c6f));
    }
}
