// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

crate::diesel_newtype! {
    #[derive(Copy)] RoomId(uuid::Uuid) => diesel::sql_types::Uuid, "/rooms/"
}
