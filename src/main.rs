mod auth;
mod db;
mod errors;
mod models;
mod routes;
mod snowflake;
mod state;
mod validation;

use std::net::SocketAddr;

use axum::{
    Router,
    http::{
        Method,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    routing::{get, post},
};
use dotenvy::dotenv;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{
    EnvFilter, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::{
    routes::{auth as auth_routes, users},
    state::AppState,
};

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
    let jwt_secret =
        std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    let access_token_expiry_minutes =
        std::env::var("ACCESS_TOKEN_EXPIRY_MINUTES")
            .expect("ACCESS_TOKEN_EXPIRY_MINUTES must be set")
            .parse::<u64>()
            .expect("ACCESS_TOKEN_EXPIRY_MINUTES could not parse u64");
    let refresh_token_expiry_days = std::env::var("REFRESH_TOKEN_EXPIRY_DAYS")
        .expect("REFRESH_TOKEN_EXPIRY_DAYS must be set")
        .parse::<u64>()
        .expect("REFRESH_TOKEN_EXPIRY_DAYS could not parse u64");
    let pool = db::create_pool(&database_url)
        .await
        .expect("Failed to connect to database");

    let state = AppState::new(
        pool,
        10,
        jwt_secret,
        access_token_expiry_minutes,
        refresh_token_expiry_days,
    );

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    let auth_governor = GovernorConfigBuilder::default()
        .per_second(1)
        .burst_size(5)
        .finish()
        .unwrap();

    let user_governor = GovernorConfigBuilder::default()
        .per_second(10)
        .burst_size(50)
        .finish()
        .unwrap();

    let auth_routes = Router::new()
        .route("/auth/register", post(auth_routes::register))
        .route("/auth/login", post(auth_routes::login))
        .route("/auth/refresh", post(auth_routes::refresh))
        .route("/auth/logout", post(auth_routes::logout))
        .layer(GovernorLayer::new(auth_governor));

    let user_routes = Router::new()
        .route("/users", get(users::list_users))
        .route(
            "/users/{id}",
            get(users::get_user)
                .put(users::update_user)
                .delete(users::delete_user),
        )
        .layer(GovernorLayer::new(user_governor));

    let app = Router::new()
        .merge(auth_routes)
        .merge(user_routes)
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("ポートのバインドに失敗しました");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("サーバーの起動に失敗しました");
}
