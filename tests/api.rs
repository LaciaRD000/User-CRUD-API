//! user-api 結合テスト
//!
//! 実行には DATABASE_URL をはじめとする環境変数（.env）が必要。
//! 未設定の場合はテストを失敗させ、CI で見逃さないようにする。

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::{get, post},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use user_api::{
    auth, db,
    routes::{auth as auth_routes, users},
    state::AppState,
};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// テストインフラ
// ---------------------------------------------------------------------------

struct TestApp {
    router: Router,
    db: PgPool,
    jwt_secret: String,
    jwt_issuer: String,
    jwt_audience: String,
    jwt_leeway_seconds: u64,
}

impl TestApp {
    async fn new() -> Self {
        dotenvy::dotenv().ok();

        fn require_env(key: &str) -> String {
            match std::env::var(key) {
                Ok(v) if !v.trim().is_empty() => v,
                _ => panic!(
                    "Missing required env var: {key} (set it or create .env)"
                ),
            }
        }

        fn require_parse_env<T>(key: &str) -> T
        where
            T: std::str::FromStr,
            T::Err: std::fmt::Display,
        {
            let v = require_env(key);
            v.trim().parse::<T>().unwrap_or_else(|err| {
                panic!("Failed to parse env var {key}={v:?}: {err}")
            })
        }

        let database_url = require_env("DATABASE_URL");
        let jwt_secret = require_env("JWT_SECRET");
        let jwt_issuer = require_env("JWT_ISSUER");
        let jwt_audience = require_env("JWT_AUDIENCE");
        let jwt_leeway_seconds: u64 = std::env::var("JWT_LEEWAY_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let access_token_expiry_minutes: u64 =
            require_parse_env("ACCESS_TOKEN_EXPIRY_MINUTES");
        let refresh_token_expiry_days: u64 =
            require_parse_env("REFRESH_TOKEN_EXPIRY_DAYS");
        let refresh_token_pepper = require_env("REFRESH_TOKEN_PEPPER");
        let snowflake_machine_id: u16 =
            require_parse_env("SNOWFLAKE_MACHINE_ID");
        let dummy_password_hash =
            bcrypt::hash("dummy-password-not-a-secret", bcrypt::DEFAULT_COST)
                .expect("Failed to generate dummy bcrypt hash in test helper");

        let pool = db::create_pool(&database_url)
            .await
            .expect("Failed to connect to database in integration tests");

        let state = AppState::new(
            pool.clone(),
            snowflake_machine_id,
            jwt_secret.clone(),
            jwt_issuer.clone(),
            jwt_audience.clone(),
            jwt_leeway_seconds,
            access_token_expiry_minutes,
            refresh_token_expiry_days,
            refresh_token_pepper,
            dummy_password_hash,
        );

        let router = Router::new()
            .route("/auth/register", post(auth_routes::register))
            .route("/auth/login", post(auth_routes::login))
            .route("/auth/refresh", post(auth_routes::refresh))
            .route("/auth/logout", post(auth_routes::logout))
            .route("/users", get(users::list_users))
            .route(
                "/users/{id}",
                get(users::get_user)
                    .put(users::update_user)
                    .delete(users::delete_user),
            )
            .with_state(state);

        Self {
            router,
            db: pool,
            jwt_secret,
            jwt_issuer,
            jwt_audience,
            jwt_leeway_seconds,
        }
    }

    /// HTTP リクエストを送信し、ステータスコードと JSON ボディを返す。
    async fn request(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json");
        if let Some(t) = token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let body = match body {
            Some(json) => Body::from(json.to_string()),
            None => Body::empty(),
        };
        let req = builder.body(body).unwrap();
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: Value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, json)
    }

    // -- 便利メソッド --

    async fn register(
        &self,
        username: &str,
        email: &str,
        password: &str,
    ) -> (StatusCode, Value) {
        self.request(
            "POST",
            "/auth/register",
            Some(json!({
                "username": username,
                "email": email,
                "password": password,
            })),
            None,
        )
        .await
    }

    async fn login(&self, email: &str, password: &str) -> (StatusCode, Value) {
        self.request(
            "POST",
            "/auth/login",
            Some(json!({
                "email": email,
                "password": password,
            })),
            None,
        )
        .await
    }

    /// アクセストークンから user_id を取得する。
    fn extract_user_id(&self, access_token: &str) -> i64 {
        let claims = auth::validate_token(
            access_token,
            &self.jwt_secret,
            &self.jwt_issuer,
            &self.jwt_audience,
            self.jwt_leeway_seconds,
        )
        .expect("Failed to validate access token in test helper");
        claims
            .sub
            .parse()
            .expect("Failed to parse user_id from sub claim")
    }

    /// テストユーザーを DB から直接削除する（クリーンアップ用）。
    async fn cleanup_user_by_email(&self, email: &str) {
        let email = email.trim().to_ascii_lowercase();
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM users WHERE email = $1")
                .bind(&email)
                .fetch_optional(&self.db)
                .await
                .ok()
                .flatten();
        if let Some((user_id,)) = row {
            sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
                .bind(user_id)
                .execute(&self.db)
                .await
                .ok();
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(user_id)
                .execute(&self.db)
                .await
                .ok();
        }
    }
}

fn unique_email() -> String { format!("test-{}@example.com", Uuid::new_v4()) }

const TEST_PASSWORD: &str = "test-password-123";

macro_rules! setup_or_skip {
    () => {
        TestApp::new().await
    };
}

// ===========================================================================
// Auth テスト
// ===========================================================================

#[tokio::test]
async fn auth_register_success() {
    let app = setup_or_skip!();
    let email = unique_email();

    let (status, body) = app.register("testuser", &email, TEST_PASSWORD).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn auth_register_invalid_username() {
    let app = setup_or_skip!();

    let (status, body) = app.register("", &unique_email(), TEST_PASSWORD).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("empty"));
}

#[tokio::test]
async fn auth_register_invalid_email() {
    let app = setup_or_skip!();

    let (status, body) = app
        .register("testuser", "not-an-email", TEST_PASSWORD)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("invalid"));
}

#[tokio::test]
async fn auth_register_invalid_password() {
    let app = setup_or_skip!();

    let (status, body) =
        app.register("testuser", &unique_email(), "short").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("short"));
}

#[tokio::test]
async fn auth_register_duplicate_email() {
    let app = setup_or_skip!();
    let email = unique_email();

    let (status, _) = app.register("user1", &email, TEST_PASSWORD).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = app.register("user2", &email, TEST_PASSWORD).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("email"));

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn auth_login_success() {
    let app = setup_or_skip!();
    let email = unique_email();

    app.register("loginuser", &email, TEST_PASSWORD).await;

    let (status, body) = app.login(&email, TEST_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn auth_login_wrong_password() {
    let app = setup_or_skip!();
    let email = unique_email();

    app.register("loginuser", &email, TEST_PASSWORD).await;

    let (status, _) = app.login(&email, "wrong-password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn auth_login_nonexistent_email() {
    let app = setup_or_skip!();

    let (status, _) = app.login("nonexistent@example.com", TEST_PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_refresh_success() {
    let app = setup_or_skip!();
    let email = unique_email();

    let (_, reg_body) =
        app.register("refreshuser", &email, TEST_PASSWORD).await;
    let refresh_token = reg_body["refresh_token"].as_str().unwrap();

    let (status, body) = app
        .request(
            "POST",
            "/auth/refresh",
            Some(json!({ "refresh_token": refresh_token })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["access_token"].is_string());
    assert!(body["refresh_token"].is_string());
    // ローテーション: 新しいリフレッシュトークンは元と異なる
    assert_ne!(body["refresh_token"].as_str().unwrap(), refresh_token);

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn auth_refresh_invalid_token() {
    let app = setup_or_skip!();

    let (status, _) = app
        .request(
            "POST",
            "/auth/refresh",
            Some(json!({ "refresh_token": "invalid-token" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_logout_success() {
    let app = setup_or_skip!();
    let email = unique_email();

    let (_, reg_body) = app.register("logoutuser", &email, TEST_PASSWORD).await;
    let access_token = reg_body["access_token"].as_str().unwrap();
    let refresh_token = reg_body["refresh_token"].as_str().unwrap();

    let (status, _) = app
        .request(
            "POST",
            "/auth/logout",
            Some(json!({ "refresh_token": refresh_token })),
            Some(access_token),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn auth_logout_without_auth() {
    let app = setup_or_skip!();

    let (status, _) = app
        .request(
            "POST",
            "/auth/logout",
            Some(json!({ "refresh_token": "some-token" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ===========================================================================
// Users テスト
// ===========================================================================

#[tokio::test]
async fn users_list_success() {
    let app = setup_or_skip!();

    let (status, body) = app.request("GET", "/users", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
}

#[tokio::test]
async fn users_list_with_pagination() {
    let app = setup_or_skip!();

    let (status, body) = app.request("GET", "/users?limit=2", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().unwrap().len() <= 2);
}

#[tokio::test]
async fn users_list_invalid_limit() {
    let app = setup_or_skip!();

    let (status, body) = app.request("GET", "/users?limit=0", None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("limit"));
}

#[tokio::test]
async fn users_get_success() {
    let app = setup_or_skip!();
    let email = unique_email();

    let (_, reg_body) = app.register("getuser", &email, TEST_PASSWORD).await;
    let token = reg_body["access_token"].as_str().unwrap();
    let user_id = app.extract_user_id(token);

    let (status, body) = app
        .request("GET", &format!("/users/{user_id}"), None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], user_id);
    assert_eq!(body["username"], "getuser");
    // password_hash はレスポンスに含まれない
    assert!(body.get("password_hash").is_none());

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn users_get_not_found() {
    let app = setup_or_skip!();

    let (status, _) = app
        .request("GET", "/users/999999999999999", None, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn users_update_own() {
    let app = setup_or_skip!();
    let email = unique_email();

    let (_, reg_body) = app.register("updateuser", &email, TEST_PASSWORD).await;
    let token = reg_body["access_token"].as_str().unwrap();
    let user_id = app.extract_user_id(token);

    let (status, body) = app
        .request(
            "PUT",
            &format!("/users/{user_id}"),
            Some(json!({ "username": "updated_name" })),
            Some(token),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "updated_name");

    app.cleanup_user_by_email(&email).await;
}

#[tokio::test]
async fn users_update_other_user_forbidden() {
    let app = setup_or_skip!();
    let email_a = unique_email();
    let email_b = unique_email();

    let (_, body_a) = app.register("user_a", &email_a, TEST_PASSWORD).await;
    let (_, body_b) = app.register("user_b", &email_b, TEST_PASSWORD).await;
    let token_a = body_a["access_token"].as_str().unwrap();
    let user_id_b =
        app.extract_user_id(body_b["access_token"].as_str().unwrap());

    // user_a のトークンで user_b を更新しようとする
    let (status, _) = app
        .request(
            "PUT",
            &format!("/users/{user_id_b}"),
            Some(json!({ "username": "hacked" })),
            Some(token_a),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    app.cleanup_user_by_email(&email_a).await;
    app.cleanup_user_by_email(&email_b).await;
}

#[tokio::test]
async fn users_update_duplicate_email_returns_409() {
    let app = setup_or_skip!();
    let email_a = unique_email();
    let email_b = unique_email();

    let (_, body_a) = app.register("user_a_dup", &email_a, TEST_PASSWORD).await;
    let (_, _body_b) =
        app.register("user_b_dup", &email_b, TEST_PASSWORD).await;
    let token_a = body_a["access_token"].as_str().unwrap();
    let user_id_a = app.extract_user_id(token_a);

    let (status, body) = app
        .request(
            "PUT",
            &format!("/users/{user_id_a}"),
            Some(json!({ "email": email_b })),
            Some(token_a),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("email"));

    app.cleanup_user_by_email(&email_a).await;
    app.cleanup_user_by_email(&email_b).await;
}

#[tokio::test]
async fn users_update_without_auth() {
    let app = setup_or_skip!();

    let (status, _) = app
        .request(
            "PUT",
            "/users/1",
            Some(json!({ "username": "hacked" })),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn users_delete_own() {
    let app = setup_or_skip!();
    let email = unique_email();

    let (_, reg_body) = app.register("deleteuser", &email, TEST_PASSWORD).await;
    let token = reg_body["access_token"].as_str().unwrap();
    let user_id = app.extract_user_id(token);

    let (status, _) = app
        .request("DELETE", &format!("/users/{user_id}"), None, Some(token))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 削除後に取得すると 404
    let (status, _) = app
        .request("GET", &format!("/users/{user_id}"), None, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn users_delete_other_user_forbidden() {
    let app = setup_or_skip!();
    let email_a = unique_email();
    let email_b = unique_email();

    let (_, body_a) = app.register("del_a", &email_a, TEST_PASSWORD).await;
    let (_, body_b) = app.register("del_b", &email_b, TEST_PASSWORD).await;
    let token_a = body_a["access_token"].as_str().unwrap();
    let user_id_b =
        app.extract_user_id(body_b["access_token"].as_str().unwrap());

    let (status, _) = app
        .request(
            "DELETE",
            &format!("/users/{user_id_b}"),
            None,
            Some(token_a),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    app.cleanup_user_by_email(&email_a).await;
    app.cleanup_user_by_email(&email_b).await;
}

#[tokio::test]
async fn users_delete_without_auth() {
    let app = setup_or_skip!();

    let (status, _) = app.request("DELETE", "/users/1", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
