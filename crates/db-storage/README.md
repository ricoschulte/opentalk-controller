# k3k-db-storage

Contains the database ORM and database migrations for the controller/storage
Builds upon k3k-database

To extend you need to implement a fitting trait extension.
Example:
```rust
use database::DbInterface;
pub trait DbExampleEx: DbInterface {
#[tracing::instrument(skip(self, new_user))]
    fn create_user(&self, new_user: ())) -> Result<()> {
        let con = self.get_con()?;
        // Do query and Return result
        Ok(())
        })
    }
}
```
