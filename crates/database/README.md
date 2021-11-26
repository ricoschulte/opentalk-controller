# k3k-database

K3K Database connector, interface and connection handling


## How-to use:
Extend the base trait here using extension traits to add functionality for specific features/tables

## Example
```rust
trait DbFeatureEx: DbInteface {
    fn feature_a(&self, x: bool) -> Result<bool>{
        let conn = self.get_con()?;
        // Do stuff with conn and x
        Ok(true)
    }
}
impl<T: DbInterface> DbFeatureEx for T {}
```
