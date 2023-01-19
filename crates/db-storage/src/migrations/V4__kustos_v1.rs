// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use barrel::{backend::Pg, Migration};

pub fn migration() -> String {
    let mut migr = Migration::new();
    // Apply kustos migration v1
    kustos::db::migrations::v1::barrel_migration(&mut migr);

    // Make the invites fkey on room(uuid) on delete cascade
    migr.change_table("invites", |table| {
        table.inject_custom("drop constraint invites_room_fkey");
        table.inject_custom(
            "add constraint invites_room_fkey
        foreign key (room)
        references rooms (uuid)
        on delete cascade
        ",
        );
    });

    migr.make::<Pg>()
}
