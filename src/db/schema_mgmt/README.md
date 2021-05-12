# How to change the schema?

* add folder `<version>_<name>` under diesel/migrations
* implement in mod.rs in the new folder:

```rust
pub fn up(migr: &mut Migration) {}

#[allow(dead_code)]
fn down(migr: &mut Migration) {}
```
* import folder in refinery/migrations/mod.rs (insert version and name):
```rust
#[path = "../../diesel/migrations/<version>_<name>/mod.rs"]
mod <version>_<name>;
```

* create `<version>__<name>.rs` in refinery/migrations
* copy/paste (insert version and name):

```rust
use barrel::backend::Pg;
use barrel::{types, Migration};
use super::<version>_<name>::up;

pub fn migration() -> String {
    let mut m = Migration::new();

    up(&mut m);

    m.make::<Pg>()
}
```

* set the environment variable `DATABASE_URL=postgres://postgres:password123@localhost/migration_test` 
* run `diesel migration run` in ./diesel
