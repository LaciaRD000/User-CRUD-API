mod auth;
mod db;
mod errors;
mod models;
mod routes;
mod snowflake;
mod state;
mod validation;

use std::{net::SocketAddr, time::Duration};

use axum::{
    Router, ServiceExt,
    extract::DefaultBodyLimit,
    http::{
        Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    routing::{get, post},
};
use dotenvy::dotenv;
use tower::{Layer, ServiceBuilder};
use tower_governor::{
    GovernorLayer,
    governor::GovernorConfigBuilder,
    key_extractor::SmartIpKeyExtractor,
};
use tower_helmet::HelmetLayer;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    normalize_path::{NormalizePath, NormalizePathLayer},
    timeout::TimeoutLayer,
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
    if jwt_secret.len() < 32 {
        panic!("JWT_SECRET must be at least 32 characters");
    }
    let jwt_issuer =
        std::env::var("JWT_ISSUER").expect("JWT_ISSUER must be set");
    let jwt_audience =
        std::env::var("JWT_AUDIENCE").expect("JWT_AUDIENCE must be set");
    let jwt_leeway_seconds = std::env::var("JWT_LEEWAY_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60);
    let access_token_expiry_minutes =
        std::env::var("ACCESS_TOKEN_EXPIRY_MINUTES")
            .expect("ACCESS_TOKEN_EXPIRY_MINUTES must be set")
            .parse::<u64>()
            .expect("ACCESS_TOKEN_EXPIRY_MINUTES could not parse u64");
    let refresh_token_expiry_days = std::env::var("REFRESH_TOKEN_EXPIRY_DAYS")
        .expect("REFRESH_TOKEN_EXPIRY_DAYS must be set")
        .parse::<u64>()
        .expect("REFRESH_TOKEN_EXPIRY_DAYS could not parse u64");
    let refresh_token_pepper = std::env::var("REFRESH_TOKEN_PEPPER")
        .expect("REFRESH_TOKEN_PEPPER must be set");
    let snowflake_machine_id = std::env::var("SNOWFLAKE_MACHINE_ID")
        .expect("SNOWFLAKE_MACHINE_ID must be set")
        .parse::<u16>()
        .expect("SNOWFLAKE_MACHINE_ID could not parse u16");
    let pool = db::create_pool(&database_url)
        .await
        .expect("Failed to connect to database");

    let state = AppState::new(
        pool,
        snowflake_machine_id,
        jwt_secret,
        jwt_issuer,
        jwt_audience,
        jwt_leeway_seconds,
        access_token_expiry_minutes,
        refresh_token_expiry_days,
        refresh_token_pepper,
    );

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE]);

    let auth_governor = GovernorConfigBuilder::default()
        .key_extractor(SmartIpKeyExtractor)
        .per_second(1)
        .burst_size(5)
        .finish()
        .unwrap();

    let user_governor = GovernorConfigBuilder::default()
        .key_extractor(SmartIpKeyExtractor)
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
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors)
                .layer(CompressionLayer::new())
                .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    Duration::from_secs(10),
                ))
                .layer(HelmetLayer::with_defaults()),
        );

    let app: NormalizePath<Router> =
        NormalizePathLayer::trim_trailing_slash().layer(app);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("ポートのバインドに失敗しました");
    axum::serve(
      listener,
      ServiceExt::<axum::http::Request<axum::body::Body>>::into_make_service_with_connect_info::<SocketAddr>(app),
    )
    .await
    .expect("サーバーの起動に失敗しました");
}
