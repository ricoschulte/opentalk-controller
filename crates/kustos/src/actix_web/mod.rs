mod middleware;

pub use middleware::KustosService;
#[derive(Clone)]
pub struct User(uuid::Uuid);

impl From<uuid::Uuid> for User {
    fn from(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}
