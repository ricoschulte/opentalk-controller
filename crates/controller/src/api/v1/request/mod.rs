// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

mod pagination;

pub(crate) use pagination::default_pagination_per_page;
pub use pagination::{CursorPaginationQuery, PagePaginationQuery};
