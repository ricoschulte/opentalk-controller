//! Success response types for REST APIv1
//!
//! These all implement the [`Responder`] trait.
//! The current Pagination support follows the GitHub REST APIv3, i.e. page hints are included inside the Link HTTP header.

use actix_web::{
    http::{header, HeaderMap},
    HttpResponse, Responder,
};
use either::Either;
use serde::Serialize;
use std::collections::HashMap;
use url::Url;

#[derive(Debug, Clone, Serialize)]
pub struct PagePaginationLinks {
    page: i64,
    first: Option<i64>,
    prev: Option<i64>,
    next: Option<i64>,
    last: Option<i64>,
}

impl PagePaginationLinks {
    pub fn new(per_page: i64, page: i64, total: i64) -> Self {
        let first = (page > 1).then(|| 1);
        let prev = (page > 1).then(|| page - 1);

        let last_page = {
            let quotient = total / per_page;
            let remainder = total % per_page;
            if (remainder > 0 && per_page > 0) || (remainder < 0 && per_page < 0) {
                quotient + 1
            } else {
                quotient
            }
        };

        let next = (page < last_page).then(|| page + 1);
        let last = (page < last_page).then(|| last_page);

        Self {
            page,
            first,
            prev,
            next,
            last,
        }
    }

    pub fn as_links_vec(&self, url: &Url) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        let mut query = url
            .query_pairs()
            .into_owned()
            .collect::<HashMap<String, String>>();
        query.remove("page");
        let mut url = url.clone();
        let base = url
            .query_pairs_mut()
            .clear()
            .extend_pairs(query.iter())
            .finish();

        if let Some(first) = self.first {
            let first = base
                .clone()
                .query_pairs_mut()
                .append_pair("page", &first.to_string())
                .finish()
                .to_string();
            headers.push(("first".to_string(), first));
        }

        if let Some(prev) = self.prev {
            let prev = base
                .clone()
                .query_pairs_mut()
                .append_pair("page", &prev.to_string())
                .finish()
                .to_string();
            headers.push(("prev".to_string(), prev));
        }

        if let Some(next) = self.next {
            let next = base
                .clone()
                .query_pairs_mut()
                .append_pair("page", &next.to_string())
                .finish()
                .to_string();
            headers.push(("next".to_string(), next));
        }
        if let Some(last) = self.last {
            let last = base
                .clone()
                .query_pairs_mut()
                .append_pair("page", &last.to_string())
                .finish()
                .to_string();
            headers.push(("last".to_string(), last));
        }
        headers
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CursorPaginationLinks {
    before: Option<String>,
    after: Option<String>,
}

impl CursorPaginationLinks {
    pub fn new<'a>(before: Option<&'a str>, after: Option<&'a str>) -> Self {
        Self {
            before: before.map(String::from),
            after: after.map(String::from),
        }
    }
    pub fn as_links_vec(&self, url: &Url) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        let mut query = url
            .query_pairs()
            .into_owned()
            .collect::<HashMap<String, String>>();
        query.remove("page");

        let mut url = url.clone();
        let base = url
            .query_pairs_mut()
            .clear()
            .extend_pairs(query.iter())
            .finish();
        if let Some(before) = &self.before {
            let before = base
                .clone()
                .query_pairs_mut()
                .append_pair("before", &before.to_string())
                .finish()
                .to_string();
            headers.push(("before".to_string(), before));
        }

        if let Some(after) = &self.after {
            let after = base
                .clone()
                .query_pairs_mut()
                .append_pair("after", &after.to_string())
                .finish()
                .to_string();
            headers.push(("after".to_string(), after));
        }
        headers
    }
}

#[derive(Debug, Clone)]
pub struct ApiOutputLinkHeader {
    pagination: Option<Either<PagePaginationLinks, CursorPaginationLinks>>,
}

#[derive(Debug, Clone)]
pub struct ApiOutputPagination {
    page: i64,
    per_page: i64,
    total: i64,
}

#[derive(Debug, Clone)]
pub struct ApiResponse<T: Serialize> {
    links: ApiOutputLinkHeader,
    data: T,
}

impl<T: Serialize> ApiResponse<T> {
    /// Creates new [`ApiResponse`]
    pub fn new(data: T) -> Self {
        Self {
            links: ApiOutputLinkHeader { pagination: None },
            data,
        }
    }

    /// Transforms [`ApiResponse`] to also return page based pagination links.
    ///
    /// This is mutually exclusive to [ApiResponse::with_cursor_pagination]
    pub fn with_page_pagination(mut self, per_page: i64, page: i64, total: i64) -> Self {
        self.links.pagination = Some(Either::Left(PagePaginationLinks::new(
            per_page, page, total,
        )));

        self
    }

    /// Transforms [`ApiResponse`] to also return cursor based pagination links
    ///
    /// This is mutually exclusive to [ApiResponse::with_page_pagination]
    #[allow(dead_code)]
    pub fn with_cursor_pagination<'a>(
        mut self,
        before: Option<&'a str>,
        after: Option<&'a str>,
    ) -> Self {
        self.links.pagination = Some(Either::Right(CursorPaginationLinks::new(before, after)));
        self
    }
}

impl<T: Serialize> Responder for ApiResponse<T> {
    fn respond_to(self, req: &actix_web::HttpRequest) -> HttpResponse {
        match serde_json::to_string(&self.data) {
            Ok(body) => {
                let url = extract_full_url_from_request(req);

                let mut headers = HeaderMap::new();
                if let Some(links) = match url {
                    Ok(url) => self.links.pagination.map(|links| {
                        links.either(
                            |l| {
                                vec_to_header_value(l.as_links_vec(&url))
                                    .expect("vec_to_header_value failed")
                            },
                            |r| {
                                vec_to_header_value(r.as_links_vec(&url))
                                    .expect("vec_to_header_value failed")
                            },
                        )
                    }),
                    Err(_) => return HttpResponse::InternalServerError().finish(),
                } {
                    headers.insert(header::LINK, links);
                }

                let mut response = HttpResponse::Ok();
                response.content_type(mime::APPLICATION_JSON);

                for pair in headers {
                    response.insert_header(pair);
                }

                response.body(body)
            }
            Err(err) => {
                HttpResponse::from_error(actix_web::error::JsonPayloadError::Serialize(err))
            }
        }
    }
}

fn vec_to_header_value(
    vec: Vec<(String, String)>,
) -> Result<header::HeaderValue, header::InvalidHeaderValue> {
    let buf = vec
        .iter()
        .map(|(rel, url)| format!("<{url}>; rel=\"{rel}\"", url = url, rel = rel))
        .collect::<Vec<_>>()
        .join(",");

    header::HeaderValue::from_str(&buf)
}

fn extract_full_url_from_request(req: &actix_web::HttpRequest) -> Result<Url, anyhow::Error> {
    let conn = req.connection_info();

    let url = Url::parse(&format!(
        "{scheme}://{host}/",
        scheme = conn.scheme(),
        host = conn.host()
    ))?;

    Ok(url.join(&req.uri().to_string())?)
}
