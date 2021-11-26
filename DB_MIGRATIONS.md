# Requirements

* install the diesel cli tool with `cargo install diesel_cli --no-default-features --features="barrel-migrations,barrel/pg,postgres"`
* make sure `rustfmt` is installed with `rustup component add rustfmt`

# How to change the schema?

* add file `V<version_nr>__<name>.rs` under crates/controller/src/db/migrations/
* implement the following structure:

```rust
pub fn migration() -> String {
    let mut migr = Migration::new();

    // database changes here

    migr.make::<Pg>()
}
```

Run `cargo xtask generate-db-schema` to generate a new diesel schema in `crates/db-storage/src/db/schema.rs`.
This creates a random database by default and deletes it afterwards.  
See `cargo xtask generate-db-schema --help` for information what options are possbile to not use default values
or specify a fixed database.

