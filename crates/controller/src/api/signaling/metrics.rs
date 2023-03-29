// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use crate::api;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};
use opentelemetry::{Context, Key};

const STARTUP_SUCCESSFUL: Key = Key::from_static_str("successful");
const DESTROY_SUCCESSFUL: Key = Key::from_static_str("successful");
const PARTICIPATION_KIND: Key = Key::from_static_str("participation_kind");
const MEDIA_SESSION_TYPE: Key = Key::from_static_str("media_session_type");

pub struct SignalingMetrics {
    pub(crate) runner_startup_time: Histogram<f64>,
    pub(crate) runner_destroy_time: Histogram<f64>,
    pub(crate) created_rooms_count: Counter<u64>,
    pub(crate) destroyed_rooms_count: Counter<u64>,
    pub(crate) participants_count: UpDownCounter<i64>,
    pub(crate) participants_with_audio_count: UpDownCounter<i64>,
    pub(crate) participants_with_video_count: UpDownCounter<i64>,
}

impl SignalingMetrics {
    pub fn record_startup_time(&self, secs: f64, success: bool) {
        self.runner_startup_time.record(
            &Context::current(),
            secs,
            &[STARTUP_SUCCESSFUL.bool(success)],
        );
    }

    pub fn record_destroy_time(&self, secs: f64, success: bool) {
        self.runner_destroy_time.record(
            &Context::current(),
            secs,
            &[DESTROY_SUCCESSFUL.bool(success)],
        );
    }

    pub fn increment_created_rooms_count(&self) {
        self.created_rooms_count.add(&Context::current(), 1, &[]);
    }

    pub fn increment_destroyed_rooms_count(&self) {
        self.destroyed_rooms_count.add(&Context::current(), 1, &[]);
    }

    pub fn increment_participants_count<U>(&self, participant: &api::Participant<U>) {
        self.participants_count.add(
            &Context::current(),
            1,
            &[PARTICIPATION_KIND.string(participant.as_kind_str())],
        );
    }

    pub fn decrement_participants_count<U>(&self, participant: &api::Participant<U>) {
        self.participants_count.add(
            &Context::current(),
            -1,
            &[PARTICIPATION_KIND.string(participant.as_kind_str())],
        );
    }

    pub fn increment_participants_with_audio_count(&self, session_type: &str) {
        self.participants_with_audio_count.add(
            &Context::current(),
            1,
            &[MEDIA_SESSION_TYPE.string(session_type.to_owned())],
        );
    }

    pub fn decrement_participants_with_audio_count(&self, session_type: &str) {
        self.participants_with_audio_count.add(
            &Context::current(),
            -1,
            &[MEDIA_SESSION_TYPE.string(session_type.to_owned())],
        );
    }

    pub fn increment_participants_with_video_count(&self, session_type: &str) {
        self.participants_with_video_count.add(
            &Context::current(),
            1,
            &[MEDIA_SESSION_TYPE.string(session_type.to_owned())],
        );
    }

    pub fn decrement_participants_with_video_count(&self, session_type: &str) {
        self.participants_with_video_count.add(
            &Context::current(),
            -1,
            &[MEDIA_SESSION_TYPE.string(session_type.to_owned())],
        );
    }
}
