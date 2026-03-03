use axum::{Json, extract::State, http::StatusCode};
use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::{
    auth::create_token,
    errors::ApiError,
    models::{
        AuthResponse, LoginUser, LogoutRequest, RefreshRequest, RegisterUser,
        User,
    },
    snowflake,
    state::AppState,
    validation::{self, validate_email, validate_password, validate_username},
};

pub async fn register(
    State(state): State<AppState>,
    body: Json<RegisterUser>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    validate_username(&body.username)
        .map_err(|err| ApiError::BadRequest(err))?;
    validate_email(&body.email).map_err(|err| ApiError::BadRequest(err))?;
    validate_password(&body.password)
        .map_err(|err| ApiError::BadRequest(err))?;

    let password_hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    let user_id = state.snowflake.lock().unwrap().generate();

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(&body.username)
    .bind(&body.email)
    .bind(password_hash)
    .execute(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    let access_token = create_token(
        user_id,
        &state.jwt_secret,
        state.access_token_expiry_minutes,
    )
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    let refresh_token = Uuid::new_v4().to_string();

    let refresh_token_id = state.snowflake.lock().unwrap().generate();

    let expires_at =
        Utc::now() + Duration::days(state.refresh_token_expiry_days as i64);

    sqlx::query("INSERT INTO refresh_tokens (id, user_id, token, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(refresh_token_id)
        .bind(user_id)
        .bind(&refresh_token)
        .bind(expires_at)
        .execute(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            access_token,
            refresh_token,
        }),
    ))
}

pub async fn login(
    State(state): State<AppState>,
    body: Json<LoginUser>,
) -> Result<Json<AuthResponse>, ApiError> {
    let user: User = sqlx::query_as(
        "SELECT id, username, email, password_hash FROM users WHERE email = $1",
    )
    .bind(&body.email)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?
    .ok_or(ApiError::Unauthorized)?;

    let is_valid = bcrypt::verify(&body.password, &user.password_hash)
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    if !is_valid {
        return Err(ApiError::Unauthorized);
    }

    let access_token = create_token(
        user.id,
        &state.jwt_secret,
        state.access_token_expiry_minutes,
    )
    .map_err(|err| ApiError::Internal(err.to_string()))?;
    let refresh_token = Uuid::new_v4().to_string();
    let refresh_token_id = state.snowflake.lock().unwrap().generate();
    let expires_at =
        Utc::now() + Duration::days(state.refresh_token_expiry_days as i64);

    sqlx::query("INSERT INTO refresh_tokens (id, user_id, token, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(refresh_token_id)
        .bind(user.id)
        .bind(&refresh_token)
        .bind(expires_at)
        .execute(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
    }))
}

pub async fn refresh(
    State(state): State<AppState>,
    body: Json<RefreshRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let user: Refre = sqlx::query_as("SELECT id, user_id, token, expires_at FROM refresh_tokens WHERE token = $1")
        .bind(&body.refresh_token)
        .fetch_optional(&state.db)
        .await
    .map_err(|err| ApiError::Internal(err.to_string()))?
    .ok_or(|_| ApiError::Unauthorized)?;
}

pub async fn logout(
    State(state): State<AppState>,
    body: Json<LogoutRequest>,
) -> Result<StatusCode, ApiError> {
}
