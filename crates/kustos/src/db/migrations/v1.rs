// SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
//
// SPDX-License-Identifier: EUPL-1.2

//! Initial addition of the underlying casbin database schema.
//! This follows the casbin guidelines naming the table `casbin_rule`
use barrel::backend::Pg;
use barrel::{types, Migration};

pub fn migration() -> String {
    let mut migr = Migration::new();
    barrel_migration(&mut migr);
    migr.make::<Pg>()
}

pub fn barrel_migration(migr: &mut Migration) {
    migr.create_table("casbin_rule", |table| {
        table.add_column("id", types::custom("SERIAL").primary(true));
        table.add_column("ptype", types::varchar(12).nullable(false));
        table.add_column("v0", types::varchar(0).nullable(false));
        table.add_column("v1", types::varchar(0).nullable(false));
        table.add_column("v2", types::varchar(0).nullable(false));
        table.add_column("v3", types::varchar(0).nullable(false));
        table.add_column("v4", types::varchar(0).nullable(false));
        table.add_column("v5", types::varchar(0).nullable(false));
        table.inject_custom(
            "CONSTRAINT unique_key_diesel_adapter UNIQUE(ptype, v0, v1, v2, v3, v4, v5)",
        );
    });
}
