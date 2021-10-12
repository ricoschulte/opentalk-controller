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
    pub per_page: u64,
    #[serde(default = "default_pagination_page")]
    pub page: u64,
}

fn default_pagination_per_page() -> u64 {
    30
}

/// Enforce the per_page setting to be <100
fn deserialize_pagination_per_page<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let per_page = u64::deserialize(deserializer)?;
    if per_page <= 100 {
        Ok(per_page)
    } else {
        Err(serde::de::Error::custom("per_page too large"))
    }
}
fn default_pagination_page() -> u64 {
    1
}

/// Cursor-based pagination query
#[derive(Deserialize)]
pub struct CursorPaginationQuery {
    pub before: Option<u64>,
    pub after: Option<u64>,
}
