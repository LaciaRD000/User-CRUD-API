use std::{net::SocketAddr, time::Duration};

use axum::{
    Router, ServiceExt,
    extract::DefaultBodyLimit,
    http::{
        HeaderName, Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    routing::{get, post},
};
use dotenvy::dotenv;
use tower::{Layer, ServiceBuilder};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tower_helmet::HelmetLayer;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    normalize_path::{NormalizePath, NormalizePathLayer},
    request_id::{MakeRequestUuid, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing_subscriber::{
    EnvFilter, layer::SubscriberExt, util::SubscriberInitExt,
};
use user_api::{
    config::Config,
    db,
    rate_limit::RateLimitIpKeyExtractor,
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

    let config = Config::from_env();

    let dummy_password_hash = {
        use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
        let salt = SaltString::generate(&mut OsRng);
        argon2::Argon2::default()
            .hash_password(b"dummy-password-not-a-secret", &salt)
            .expect("Failed to generate dummy argon2 hash")
            .to_string()
    };
    let pool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to database");

    let state = AppState::new(
        pool,
        config.snowflake_machine_id,
        config.jwt_secret,
        config.jwt_issuer,
        config.jwt_audience,
        config.jwt_leeway_seconds,
        config.access_token_expiry_minutes,
        config.refresh_token_expiry_days,
        config.refresh_token_pepper,
        dummy_password_hash,
    );

    let x_request_id = HeaderName::from_static("x-request-id");

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE])
        .expose_headers([x_request_id.clone()]);

    let key_extractor = RateLimitIpKeyExtractor::new(config.rate_limit_ip_mode);

    let auth_governor = GovernorConfigBuilder::default()
        .key_extractor(key_extractor)
        .per_second(1)
        .burst_size(5)
        .finish()
        .unwrap();

    let user_governor = GovernorConfigBuilder::default()
        .key_extractor(key_extractor)
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
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|req: &axum::http::Request<_>| {
                            let request_id = req
                                .headers()
                                .get("x-request-id")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("-");
                            tracing::info_span!(
                                "request",
                                method = %req.method(),
                                uri = %req.uri(),
                                request_id = %request_id,
                            )
                        })
                        .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
                )
                .layer(cors)
                .layer(CompressionLayer::new())
                .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    Duration::from_secs(10),
                ))
                .layer(HelmetLayer::with_defaults())
                .layer(SetRequestIdLayer::new(
                    x_request_id.clone(),
                    MakeRequestUuid,
                )),
        );

    let app: NormalizePath<Router> =
        NormalizePathLayer::trim_trailing_slash().layer(app);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("ポートのバインドに失敗しました");

    tracing::info!("Listening on 0.0.0.0:3000");

    axum::serve(
      listener,
      ServiceExt::<axum::http::Request<axum::body::Body>>::into_make_service_with_connect_info::<SocketAddr>(app),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("サーバーの起動に失敗しました");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("Failed to install SIGTERM handler")
        .recv()
        .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("Shutdown signal received, finishing in-flight requests...");
}
