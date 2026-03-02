use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

#[derive(Clone, Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
}

#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub email: String,
}

#[derive(Deserialize)]
pub struct UpdateUser {
    pub username: Option<String>,
    pub email: Option<String>,
}
