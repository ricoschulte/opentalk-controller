// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Internal API
//!
//! Current Endpoints. See their respective function:
//! - `/rooms/{room_id}` ([DELETE](rooms::delete)

use actix_http::body::BoxBody;
use actix_web::{HttpResponse, Responder};

pub mod rooms;

/// Represents a 204 No Content HTTP Response
pub struct NoContent;

impl Responder for NoContent {
    type Body = BoxBody;

    fn respond_to(self, _: &actix_web::HttpRequest) -> HttpResponse {
        HttpResponse::NoContent().finish()
    }
}
