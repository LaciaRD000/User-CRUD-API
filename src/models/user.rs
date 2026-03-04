use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

#[derive(Clone, Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    pub email: String,

    #[serde(skip_serializing)]
    #[sqlx(default)]
    pub password_hash: String,
}

#[derive(Clone, Serialize, FromRow)]
pub struct PublicUser {
    pub id: i64,
    pub username: String,
}

#[derive(Deserialize)]
pub struct UpdateUser {
    pub username: Option<String>,
    pub email: Option<String>,
}
