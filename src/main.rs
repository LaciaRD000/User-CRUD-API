mod auth;
mod db;
mod errors;
mod models;
mod routes;
mod snowflake;
mod state;
mod validation;

use axum::{
    Router,
    http::Method,
    routing::{get, post},
};
use dotenvy::dotenv;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{
    EnvFilter, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::{routes::users, state::AppState};

#[tokio::main]
async fn main() {
    let _ = dotenv().expect(".env file not found");

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME"))
                .into()
        }))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = db::create_pool(&database_url)
        .await
        .expect("Failed to connect to database");

    let state = AppState::new(pool, 10);

    let cors = CorsLayer::new().allow_origin(Any).allow_methods([
        Method::GET,
        Method::POST,
        Method::PUT,
        Method::DELETE,
    ]);

    let app = Router::new()
        .route("/users", get(users::list_users))
        .route(
            "/users/{id}",
            get(users::get_user)
                .put(users::update_user)
                .delete(users::delete_user),
        )
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("ポートのバインドに失敗しました");

    axum::serve(listener, app)
        .await
        .expect("サーバーの起動に失敗しました");
}
