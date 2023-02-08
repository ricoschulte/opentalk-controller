// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use bytes::Bytes;

use anyhow::{bail, Context, Result};
use futures::Stream;
use reqwest::header::COOKIE;
use reqwest::{Client, Response, StatusCode, Url};
use serde::Deserialize;
use serde_repr::Deserialize_repr;

#[derive(Clone)]
/// The client for the etherpad API
pub struct EtherpadClient {
    /// reqwest client
    client: Client,
    /// The base url of the etherpad
    base_url: Url,
    /// The API key is set by the etherpad instance
    api_key: String,
}

#[derive(Debug, Deserialize)]
/// A response from the etherpad API
struct EtherpadResponse {
    /// API response code
    code: ResponseCode,
    /// Status message for error information
    message: String,
    /// The returned JSON data
    data: serde_json::Value,
}

impl EtherpadResponse {
    /// Get the string value behind the provided `key` from the etherpad response data
    fn get_str_data(&self, key: &str) -> Result<&str> {
        let str_data = self
            .data
            .get(key)
            .with_context(|| {
                format!(
                    "Missing data '{}' in etherpad response. Got: {}",
                    key, self.data
                )
            })?
            .as_str()
            .with_context(|| {
                format!(
                    "Data '{}' in etherpad response is not a string. Got:  {:?}",
                    key,
                    self.data.get(key)
                )
            })?;
        Ok(str_data)
    }
}

#[derive(Debug, Deserialize_repr)]
#[repr(i64)]
/// The etherpad response code for API request
enum ResponseCode {
    Ok = 0,
    WrongParameters,
    InternalError,
    NoSuchFunction,
    InvalidApiKey,
}

impl EtherpadClient {
    /// Create a new etherpad client
    pub fn new(base_url: Url, api_key: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
            api_key,
        }
    }

    /// Create a new etherpad author mapped to an internal id
    ///
    /// When the mapped id already exists in the etherpad db, the author name
    /// gets updated.
    pub async fn create_author_if_not_exits_for(
        &self,
        author_name: &str,
        mapped_id: &str,
    ) -> Result<String> {
        let mut url = self.base_url.join("api/1/createAuthorIfNotExistsFor")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("name", author_name)
            .append_pair("authorMapper", mapped_id);

        let response = self.client.get(url).send().await?;

        let response = verify_etherpad_response(response)
            .await
            .context("Failed to call etherpad endpoint 'createAuthorIfNotExistsFor'")?;

        let author_id = response.get_str_data("authorID")?;

        Ok(author_id.into())
    }

    /// Create a new etherpad group mapped to an internal id
    pub async fn create_group_for(&self, mapped_id: String) -> Result<String> {
        let mut url = self.base_url.join("api/1/createGroupIfNotExistsFor")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("groupMapper", &mapped_id);

        let response = self.client.get(url).send().await?;

        let response = verify_etherpad_response(response)
            .await
            .context("Failed to call etherpad endpoint 'createGroupIfNotExistsFor'")?;

        let group_id = response.get_str_data("groupID")?;

        Ok(group_id.into())
    }

    pub async fn delete_group(&self, group_id: &str) -> Result<()> {
        let mut url = self.base_url.join("readSession/deleteGroup")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("groupID", group_id);

        let response = self.client.get(url).send().await?;

        verify_etherpad_response(response)
            .await
            .context("Failed to call etherpad endpoint 'deleteGroup'")?;

        Ok(())
    }

    pub async fn create_group_pad(
        &self,
        group_id: &str,
        pad_name: &str,
        text: Option<String>,
    ) -> Result<()> {
        let mut url = self.base_url.join("api/1/createGroupPad")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("groupID", group_id)
            .append_pair("padName", pad_name);

        let mut request = self.client.post(url);

        if let Some(text) = text {
            request = request.body(text);
        }

        let response = request.send().await?;

        verify_etherpad_response(response)
            .await
            .context("Failed to call etherpad endpoint 'createGroupPad'")?;

        Ok(())
    }

    pub async fn delete_pad(&self, pad_id: &str) -> Result<()> {
        let mut url = self.base_url.join("api/1/deletePad")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("padID", pad_id);

        let response = self.client.get(url).send().await?;

        verify_etherpad_response(response)
            .await
            .context("Failed to call etherpad endpoint 'deletePad'")?;

        Ok(())
    }

    pub async fn get_readonly_pad_id(&self, pad_id: &str) -> Result<String> {
        let mut url = self.base_url.join("api/1/getReadOnlyID")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("padID", pad_id);

        let response = self.client.get(url).send().await?;

        let response = verify_etherpad_response(response)
            .await
            .context("Failed to call etherpad endpoint 'getReadOnlyID'")?;

        let readonly_id = response.get_str_data("readOnlyID")?;

        Ok(readonly_id.into())
    }

    pub async fn create_session(
        &self,
        group_id: &str,
        author_id: &str,
        expires: i64,
    ) -> Result<String> {
        let mut url = self.base_url.join("api/1/createSession")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("groupID", group_id)
            .append_pair("authorID", author_id)
            .append_pair("validUntil", &expires.to_string());

        let response = self.client.get(url).send().await?;

        let response = verify_etherpad_response(response).await?;

        let session_id = response
            .get_str_data("sessionID")
            .context("Failed to call etherpad endpoint 'createSession'")?;

        Ok(session_id.into())
    }

    pub async fn create_read_session(
        &self,
        group_id: &str,
        author_id: &str,
        expires: i64,
    ) -> Result<String> {
        let mut url = self.base_url.join("readSession/createReadSession")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("groupID", group_id)
            .append_pair("authorID", author_id)
            .append_pair("validUntil", &expires.to_string());

        let response = self.client.get(url).send().await?;

        let response = verify_etherpad_response(response).await?;

        let session_id = response
            .get_str_data("sessionID")
            .context("Failed to call etherpad endpoint 'createReadSession'")?;

        Ok(session_id.into())
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let mut url = self.base_url.join("readSession/deleteSession")?;

        url.query_pairs_mut()
            .append_pair("apikey", &self.api_key)
            .append_pair("sessionID", session_id);

        let response = self.client.get(url).send().await?;

        verify_etherpad_response(response).await?;

        Ok(())
    }

    /// Download the current content of the pad as PDF document
    ///
    /// Because this is not an API endpoint, a session id has to be provided.
    ///
    /// Returns the PDF document as bytes
    pub async fn download_pdf(
        &self,
        session_id: &str,
        readonly_pad_id: &str,
    ) -> Result<impl Stream<Item = reqwest::Result<Bytes>> + std::marker::Unpin> {
        let url = self
            .base_url
            .join(&format!("p/{readonly_pad_id}/export/pdf"))?;

        let cookie_value = format!("sessionID={session_id}; path=/; SameSite=None; Secure;");

        let response = self
            .client
            .get(url)
            .header(COOKIE, cookie_value)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => (),
            error_status => bail!(
                "Failed to export pad as PDF document, got StatusCode :{}",
                error_status
            ),
        }

        Ok(response.bytes_stream())
    }

    /// The auth_session endpoint sets the session cookie on the client browser and forwards
    /// to the pad that was supplied in the query string
    ///
    /// Note: Requires the `ep_auth_session` plugin to be installed on the etherpad instance
    ///
    /// Returns the auth_session url to be called by the client
    pub fn auth_session_url(
        &self,
        session_id: &str,
        pad_name: &str,
        group_id: Option<&str>,
    ) -> Result<Url> {
        let mut url = self.base_url.join("auth_session")?;

        url.query_pairs_mut()
            .append_pair("sessionID", session_id)
            .append_pair("padName", pad_name);

        if let Some(group_id) = group_id {
            url.query_pairs_mut().append_pair("groupID", group_id);
        }

        Ok(url)
    }
}

async fn verify_etherpad_response(response: Response) -> Result<EtherpadResponse> {
    let etherpad_response = response.json::<EtherpadResponse>().await?;

    match etherpad_response.code {
        ResponseCode::Ok => (),
        failed => {
            bail!(
                "Non-success response from etherpad: {:?}, {}",
                failed,
                etherpad_response.message
            );
        }
    }

    Ok(etherpad_response)
}
