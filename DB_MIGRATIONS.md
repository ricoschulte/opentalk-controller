# Requirements

* install the diesel cli tool with `cargo install diesel_cli --no-default-features --features="barrel-migrations,barrel/pg,postgres"`
* make sure `rustfmt` is installed with `rustup component add rustfmt`

# How to change the schema?

* add file `V<version_nr>__<name>.rs` under src/db/migrations/
* implement the following structure:

```rust
pub fn migration() -> String {
    let mut migr = Migration::new();

    // database changes here

    migr.make::<Pg>()
}
```

* set the environment variable `DATABASE_URL=postgres://postgres:password123@localhost/k3k`
* run the migration via `cargo test`
* run the `generate_schema.sh` in crate root to generate a new diesel schema in `src/db/schema.rs`
