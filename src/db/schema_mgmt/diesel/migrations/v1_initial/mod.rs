use barrel::{types, Migration};

/// Handle up schema_mgmt
pub fn up(migr: &mut Migration) {
    migr.create_table("users", |table| {
        table.add_column("id", types::custom("BIGSERIAL").primary(true));
        table.add_column("oidc_uuid", types::uuid());
        table.add_column("email", types::varchar(255).unique(true).nullable(false));
        table.add_column("title", types::varchar(255).nullable(false));
        table.add_column("firstname", types::varchar(255).nullable(false));
        table.add_column("lastname", types::varchar(255).nullable(false));
        table.add_column("id_token_exp", types::custom("BIGINT").nullable(false));
        table.add_column("theme", types::varchar(255));
        table.add_column("language", types::varchar(35).nullable(false));
    });

    migr.create_table("rooms", |table| {
        table.add_column("id", types::custom("BIGSERIAL").primary(true));
        table.add_column("uuid", types::uuid());
        table.add_column("owner", types::custom("BIGINT REFERENCES users(id)"));
        table.add_column("password", types::varchar(255));
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
