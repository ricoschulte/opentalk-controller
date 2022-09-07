//! Pagination Query types
//!
//! Great blogposts are: <https://phauer.com/2015/restful-api-design-best-practices/> and <https://phauer.com/2018/web-api-pagination-timestamp-id-continuation-token/>
use serde::Deserialize;

/// Page-based pagination query
#[derive(Deserialize)]
pub struct PagePaginationQuery {
    #[serde(
        default = "default_pagination_per_page",
        deserialize_with = "deserialize_pagination_per_page"
    )]
    pub per_page: i64,
    #[serde(
        default = "default_pagination_page",
        deserialize_with = "deserialize_pagination_page"
    )]
    pub page: i64,
}

pub(crate) const fn default_pagination_per_page() -> i64 {
    30
}

/// Enforce the per_page setting to be <=100 and >0
fn deserialize_pagination_per_page<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let per_page = i64::deserialize(deserializer)?;
    if per_page <= 100 && per_page > 0 {
        Ok(per_page)
    } else if per_page <= 0 {
        Err(serde::de::Error::custom("per_page <= 0"))
    } else {
        Err(serde::de::Error::custom("per_page too large"))
    }
}

pub(crate) const fn default_pagination_page() -> i64 {
    1
}

/// Enforce the page setting to be >0
fn deserialize_pagination_page<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let page = i64::deserialize(deserializer)?;
    if page > 0 {
        Ok(page)
    } else {
        Err(serde::de::Error::custom("page too large"))
    }
}
/// Cursor-based pagination query
#[derive(Deserialize)]
pub struct CursorPaginationQuery {
    pub before: Option<u64>,
    pub after: Option<u64>,
}
