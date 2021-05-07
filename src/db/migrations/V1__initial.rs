use barrel::backend::Pg;
use barrel::{types, Migration};

pub fn migration() -> String {
    let mut m = Migration::new();

    m.create_table("users", |table| {
        table.add_column("oidc-uuid", types::uuid().primary(true));
        table.add_column("mail", types::varchar(320).unique(true).nullable(false));
    });

    m.make::<Pg>()
}
