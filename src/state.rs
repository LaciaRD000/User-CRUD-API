use crate::snowflake::SnowflakeGenerator;
use sqlx::PgPool;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub snowflake: Arc<Mutex<SnowflakeGenerator>>,
}

impl AppState {
    pub fn new(pool: PgPool, machine_id: u16) -> Self {
        Self {
            db: pool,
            snowflake: Arc::new(Mutex::new(SnowflakeGenerator::new(machine_id))),
        }
    }
}
