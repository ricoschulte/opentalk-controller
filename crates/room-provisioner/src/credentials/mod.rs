use anyhow::Result;
use controller::settings::Database;

mod helper;
mod postgresql;

pub struct Credentials {
    pub postgresql: Database,
}

impl Credentials {
    pub fn from_environment() -> Result<Self> {
        let credentials = Self {
            postgresql: postgresql::from_environment()?,
        };
        Ok(credentials)
    }
}
