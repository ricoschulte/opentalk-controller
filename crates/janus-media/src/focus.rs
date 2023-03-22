// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use std::collections::VecDeque;
use types::core::ParticipantId;

#[derive(Default)]
pub struct FocusDetection {
    queue: VecDeque<ParticipantId>,
    last_focus: Option<ParticipantId>,
}

impl FocusDetection {
    pub fn on_started_talking(&mut self, id: ParticipantId) -> Option<Option<ParticipantId>> {
        self.remove(id);
        self.queue.push_back(id);
        self.get_focussed_update()
    }

    pub fn on_stopped_talking(&mut self, id: ParticipantId) -> Option<Option<ParticipantId>> {
        self.remove(id);
        self.get_focussed_update()
    }

    fn remove(&mut self, id: ParticipantId) {
        if let Some(i) = self.queue.iter().position(|other| other == &id) {
            self.queue.remove(i);
        }
    }

    fn get_focussed_update(&mut self) -> Option<Option<ParticipantId>> {
        let focus = self.queue.front().copied();

        if focus != self.last_focus {
            self.last_focus = focus;
            Some(focus)
        } else {
            None
        }
    }
}
