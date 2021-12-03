use barrel::backend::Pg;
use barrel::{types, Migration};

pub fn migration() -> String {
    let mut migr = Migration::new();

    migr.create_table("legal_vote_room", |table| {
        table.add_column(
            "vote_id",
            types::custom("UUID REFERENCES legal_votes(id) ON DELETE CASCADE").primary(true),
        );

        table.add_column(
            "room_id",
            types::custom("UUID REFERENCES rooms(uuid) ON DELETE CASCADE"),
        );
    });

    migr.make::<Pg>()
}
