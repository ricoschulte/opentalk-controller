//! This module contains all database records as structs.
//!
//! The Object-relational mapping is done via the `CRUDTable` trait from Rbatis
use rbatis::crud_enable;
use uuid::Uuid;

/// Represents a record in the `users` table.
#[crud_enable(id_name: "oidc_uuid" | formats_pg: "oidc_uuid:{}::uuid" | table_name: "users")]
#[derive(Clone, Debug)]
pub struct User {
    pub oidc_uuid: Option<Uuid>,
    pub mail: Option<String>,
}
