//! Implementation of opaque cursor for pagination

use serde::de::{DeserializeOwned, Error, Unexpected, Visitor};
use serde::Deserialize;
use serde::Serialize;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

/// Opaque token which represents T as a base64 string (where T is encoded using bincode)
///
/// Used for cursor based pagination
#[derive(Debug, Copy, Clone)]
pub struct Cursor<T>(pub T);

impl<T> Cursor<T>
where
    T: Serialize,
{
    /// Encode T using bincode and return it as base64 string
    pub fn to_base64(&self) -> String {
        base64::encode_config(
            bincode::serialize(&self.0).unwrap(),
            base64::URL_SAFE_NO_PAD,
        )
    }
}

impl<T> Deref for Cursor<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Cursor<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Serialize for Cursor<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let cursor = self.to_base64();
        serializer.serialize_str(&cursor)
    }
}

impl<'de, T> Deserialize<'de> for Cursor<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(CursorVisitor::<T>(PhantomData))
    }
}

struct CursorVisitor<T>(PhantomData<T>);
impl<'de, T> Visitor<'de> for CursorVisitor<T>
where
    T: DeserializeOwned,
{
    type Value = Cursor<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "base64 + bincode encoded cursor data")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let bytes = base64::decode_config(v, base64::URL_SAFE_NO_PAD)
            .map_err(|_| Error::invalid_value(Unexpected::Str(v), &self))?;
        let data = bincode::deserialize(&bytes)
            .map_err(|_| Error::invalid_value(Unexpected::Bytes(&bytes), &self))?;

        Ok(Cursor(data))
    }
}
