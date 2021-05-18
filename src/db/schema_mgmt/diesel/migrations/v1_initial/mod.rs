use barrel::{types, Migration};

/// Handle up schema_mgmt
pub fn up(migr: &mut Migration) {
    migr.create_table("users", |table| {
        table.add_column("id", types::custom("BIGSERIAL").primary(true));
        table.add_column("oidc_uuid", types::uuid());
        table.add_column("email", types::varchar(320).unique(true).nullable(false));
    });

    migr.create_table("rooms", |table| {
        table.add_column("id", types::custom("BIGSERIAL").primary(true));
        table.add_column("owner", types::custom("BIGINT REFERENCES users(id)"));
        table.add_column("password", types::varchar(128));
        table.add_column("wait_for_moderator", types::boolean().nullable(false));
        table.add_column("listen_only", types::boolean().nullable(false));
    });
}

/// Handle down schema_mgmt
#[allow(dead_code)]
fn down(migr: &mut Migration) {
    migr.drop_table("rooms");
    migr.drop_table("users");
}
