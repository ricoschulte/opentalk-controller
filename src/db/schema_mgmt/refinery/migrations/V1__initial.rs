use super::v1_diesel::up;
use barrel::backend::Pg;
use barrel::Migration;

pub fn migration() -> String {
    let mut m = Migration::new();

    up(&mut m);

    m.make::<Pg>()
}
