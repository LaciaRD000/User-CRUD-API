use sqlx::{PgPool, postgres::PgPoolOptions};
use std::env;

pub async fn create_pool() -> Result<PgPool, sqlx::Error> {
    let url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPoolOptions::new().max_connections(5).connect(&url).await
}
