use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

#[derive(Deserialize)]
pub struct RegisterUser {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginUser {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(FromRow)]
pub struct RefreshToken {
    #[allow(dead_code)]
    pub id: i64,
    pub user_id: i64,
    #[allow(dead_code)]
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}
