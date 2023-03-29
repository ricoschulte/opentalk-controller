// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use opentelemetry::metrics::Histogram;

pub struct KustosMetrics {
    pub enforce_execution_time: Histogram<f64>,
    pub load_policy_execution_time: Histogram<f64>,
}
