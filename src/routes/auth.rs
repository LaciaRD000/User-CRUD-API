use axum::{Json, extract::State, http::StatusCode};
use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::{
    auth::{Claims, create_token},
    errors::ApiError,
    models::{
        AuthResponse, LoginUser, LogoutRequest, RefreshRequest, RefreshToken,
        RegisterUser, User,
    },
    state::AppState,
    validation::{normalize_email, validate_email, validate_password, validate_username},
};

const USERS_EMAIL_UNIQUE_CONSTRAINT: &str = "users_email_key";
const USERS_EMAIL_LOWER_UNIQUE_INDEX: &str = "users_email_lower_key";

fn map_unique_violation_to_conflict(
    code: Option<&str>,
    constraint: Option<&str>,
) -> Option<ApiError> {
    // Postgres unique_violation is SQLSTATE 23505.
    if code != Some("23505") {
        return None;
    }
    if constraint == Some(USERS_EMAIL_UNIQUE_CONSTRAINT)
        || constraint == Some(USERS_EMAIL_LOWER_UNIQUE_INDEX)
    {
        return Some(ApiError::Conflict("email already exists".into()));
    }
    None
}

fn hash_refresh_token(token: &str, pepper: &str) -> Result<String, ApiError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(pepper.as_bytes())
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    mac.update(token.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

async fn issue_refresh_token(
    state: &AppState,
    user_id: i64,
) -> Result<String, ApiError> {
    let token = Uuid::new_v4().to_string();
    let token_hash = hash_refresh_token(&token, &state.refresh_token_pepper)?;
    let id = state.snowflake.lock().unwrap().generate();
    let expires_at =
        Utc::now() + Duration::days(state.refresh_token_expiry_days as i64);

    sqlx::query("INSERT INTO refresh_tokens (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(user_id)
        .bind(&token_hash)
        .bind(expires_at)
        .execute(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(token)
}

pub async fn register(
    State(state): State<AppState>,
    body: Json<RegisterUser>,
) -> Result<(StatusCode, Json<AuthResponse>), ApiError> {
    validate_username(&body.username)
        .map_err(|err| ApiError::BadRequest(err))?;
    let email = normalize_email(&body.email);
    validate_email(&email).map_err(|err| ApiError::BadRequest(err))?;
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
    .bind(&email)
    .bind(password_hash)
    .execute(&state.db)
    .await
    .map_err(|err| {
        if let Some(db_err) = err.as_database_error() {
            if let Some(api_err) = map_unique_violation_to_conflict(
                db_err.code().as_deref(),
                db_err.constraint(),
            ) {
                return api_err;
            }
        }
        ApiError::Internal(err.to_string())
    })?;

    let access_token = create_token(
        user_id,
        &state.jwt_secret,
        &state.jwt_issuer,
        &state.jwt_audience,
        state.access_token_expiry_minutes,
    )
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    let refresh_token = issue_refresh_token(&state, user_id).await?;

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
    let email = normalize_email(&body.email);
    let user: User = sqlx::query_as(
        "SELECT id, username, email, password_hash FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?
    .ok_or(ApiError::Unauthorized)?;

    let is_valid = bcrypt::verify(&body.password, &user.password_hash)
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    if !is_valid {
        return Err(ApiError::Unauthorized);
    }

    sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
        .bind(user.id)
        .execute(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    let access_token = create_token(
        user.id,
        &state.jwt_secret,
        &state.jwt_issuer,
        &state.jwt_audience,
        state.access_token_expiry_minutes,
    )
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    let refresh_token = issue_refresh_token(&state, user.id).await?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
    }))
}

pub async fn refresh(
    State(state): State<AppState>,
    body: Json<RefreshRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    let token_hash =
        hash_refresh_token(&body.refresh_token, &state.refresh_token_pepper)?;

    let user: RefreshToken = sqlx::query_as("SELECT id, user_id, token_hash, expires_at FROM refresh_tokens WHERE token_hash = $1")
        .bind(&token_hash)
        .fetch_optional(&state.db)
        .await
    .map_err(|err| ApiError::Internal(err.to_string()))?
    .ok_or(ApiError::Unauthorized)?;

    if user.expires_at < Utc::now() {
        sqlx::query(
            "DELETE FROM refresh_tokens WHERE token_hash = $1 AND user_id = $2",
        )
        .bind(&token_hash)
        .bind(user.user_id)
        .execute(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
        return Err(ApiError::Unauthorized);
    }

    sqlx::query(
        "DELETE FROM refresh_tokens WHERE token_hash = $1 AND user_id = $2",
    )
    .bind(&token_hash)
    .bind(user.user_id)
    .execute(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    let access_token = create_token(
        user.user_id,
        &state.jwt_secret,
        &state.jwt_issuer,
        &state.jwt_audience,
        state.access_token_expiry_minutes,
    )
    .map_err(|err| ApiError::Internal(err.to_string()))?;
    let refresh_token = issue_refresh_token(&state, user.user_id).await?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    claims: Claims,
    body: Json<LogoutRequest>,
) -> Result<StatusCode, ApiError> {
    let user_id = claims
        .sub
        .parse::<i64>()
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    let token_hash =
        hash_refresh_token(&body.refresh_token, &state.refresh_token_pepper)?;

    sqlx::query(
        "DELETE FROM refresh_tokens WHERE token_hash = $1 AND user_id = $2",
    )
    .bind(&token_hash)
    .bind(&user_id)
    .execute(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_token_hash_depends_on_pepper() {
        let token = "550e8400-e29b-41d4-a716-446655440000";
        let a = hash_refresh_token(token, "pepper-a").unwrap();
        let b = hash_refresh_token(token, "pepper-b").unwrap();
        assert_ne!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn register_unique_violation_maps_only_email_constraint_to_conflict() {
        assert!(matches!(
            map_unique_violation_to_conflict(
                Some("23505"),
                Some(USERS_EMAIL_UNIQUE_CONSTRAINT)
            ),
            Some(ApiError::Conflict(_))
        ));
        assert!(matches!(
            map_unique_violation_to_conflict(
                Some("23505"),
                Some(USERS_EMAIL_LOWER_UNIQUE_INDEX)
            ),
            Some(ApiError::Conflict(_))
        ));
        assert!(map_unique_violation_to_conflict(
            Some("23505"),
            Some("other_unique")
        )
        .is_none());
        assert!(map_unique_violation_to_conflict(
            Some("99999"),
            Some(USERS_EMAIL_UNIQUE_CONSTRAINT)
        )
        .is_none());
    }
}
