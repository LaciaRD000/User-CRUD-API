use crate::snowflake::SnowflakeGenerator;
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub snowflake: Arc<Mutex<SnowflakeGenerator>>,
    pub jwt_secret: String,
    pub access_token_expiry_minutes: u64,
    pub refresh_token_expiry_days: u64,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        machine_id: u16,
        jwt_secret: String,
        access_token_expiry_minutes: u64,
        refresh_token_expiry_days: u64,
    ) -> Self {
        Self {
            db: pool,
            snowflake: Arc::new(Mutex::new(SnowflakeGenerator::new(
                machine_id,
            ))),
            jwt_secret,
            access_token_expiry_minutes,
            refresh_token_expiry_days,
        }
    }
}
