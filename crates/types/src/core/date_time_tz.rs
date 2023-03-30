// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use chrono::{DateTime, Utc};

use crate::{core::TimeZone, imports::*};

/// Representation of a datetime timezone
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
pub struct DateTimeTz {
    /// UTC datetime
    pub datetime: DateTime<Utc>,
    /// Timezone in which the datetime was created in
    pub timezone: TimeZone,
}
