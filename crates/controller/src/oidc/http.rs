// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use openidconnect::reqwest::Error;
use openidconnect::{HttpRequest, HttpResponse};
use std::future::Future;
use std::pin::Pin;

pub fn make_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

pub fn async_http_client(
    client: reqwest::Client,
) -> impl Fn(HttpRequest) -> Pin<Box<dyn Future<Output = Result<HttpResponse, Error<reqwest::Error>>>>>
{
    move |request| Box::pin(async_http_client_inner(client.clone(), request))
}

async fn async_http_client_inner(
    client: reqwest::Client,
    request: HttpRequest,
) -> Result<HttpResponse, Error<reqwest::Error>> {
    let mut request_builder = client
        .request(request.method, request.url.as_str())
        .body(request.body);
    for (name, value) in &request.headers {
        request_builder = request_builder.header(name.as_str(), value.as_bytes());
    }
    let request = request_builder.build().map_err(Error::Reqwest)?;

    let response = client.execute(request).await.map_err(Error::Reqwest)?;

    let status_code = response.status();
    let headers = response.headers().to_owned();
    let chunks = response.bytes().await.map_err(Error::Reqwest)?;
    Ok(HttpResponse {
        status_code,
        headers,
        body: chunks.to_vec(),
    })
}
