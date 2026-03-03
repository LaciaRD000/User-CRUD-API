use std::sync::{Arc, Mutex};

use sqlx::PgPool;

use crate::snowflake::SnowflakeGenerator;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub snowflake: Arc<Mutex<SnowflakeGenerator>>,
    pub jwt_secret: String,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub jwt_leeway_seconds: u64,
    pub access_token_expiry_minutes: u64,
    pub refresh_token_expiry_days: u64,
    pub refresh_token_pepper: String,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        machine_id: u16,
        jwt_secret: String,
        jwt_issuer: String,
        jwt_audience: String,
        jwt_leeway_seconds: u64,
        access_token_expiry_minutes: u64,
        refresh_token_expiry_days: u64,
        refresh_token_pepper: String,
    ) -> Self {
        Self {
            db: pool,
            snowflake: Arc::new(Mutex::new(SnowflakeGenerator::new(
                machine_id,
            ))),
            jwt_secret,
            jwt_issuer,
            jwt_audience,
            jwt_leeway_seconds,
            access_token_expiry_minutes,
            refresh_token_expiry_days,
            refresh_token_pepper,
        }
    }
}
