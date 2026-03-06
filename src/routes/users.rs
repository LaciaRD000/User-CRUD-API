use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    auth::Claims,
    errors::ApiError,
    models::{PublicUser, UpdateUser},
    state::AppState,
    validation::{normalize_email, validate_email, validate_username},
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

#[derive(Deserialize)]
pub struct UsersPagination {
    pub limit: Option<i64>,
    pub after_id: Option<i64>,
}

pub async fn list_users(
    State(state): State<AppState>,
    Query(pagination): Query<UsersPagination>,
) -> Result<Json<Vec<PublicUser>>, ApiError> {
    let limit = pagination.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::BadRequest("limit is out of range".into()));
    }

    let users: Vec<PublicUser> = match pagination.after_id {
        Some(after_id) => {
            if after_id < 0 {
                return Err(ApiError::BadRequest(
                    "after_id must be >= 0".into(),
                ));
            }
            sqlx::query_as(
                "SELECT id, username FROM users WHERE id > $1 ORDER BY id LIMIT $2",
            )
            .bind(after_id)
            .bind(limit)
            .fetch_all(&state.db)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?
        }
        None => sqlx::query_as(
            "SELECT id, username FROM users ORDER BY id LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?,
    };
    Ok(Json(users))
}

pub async fn get_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<PublicUser>, ApiError> {
    let user: Option<PublicUser> =
        sqlx::query_as("SELECT id, username FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?;

    match user {
        Some(user) => Ok(Json(user)),
        None => Err(ApiError::NotFound),
    }
}

pub async fn update_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    claims: Claims,
    body: Json<UpdateUser>,
) -> Result<Json<PublicUser>, ApiError> {
    let auth_user_id = claims
        .sub
        .parse::<i64>()
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    if auth_user_id != user_id {
        return Err(ApiError::Forbidden);
    }

    if let Some(ref username) = body.username {
        validate_username(username).map_err(ApiError::BadRequest)?;
    }

    if let Some(ref email) = body.email {
        let email = normalize_email(email);
        validate_email(&email).map_err(ApiError::BadRequest)?;
    }

    let user: PublicUser = sqlx::query_as("UPDATE users SET username = COALESCE($1, username), email = COALESCE($2, email) where id = $3 RETURNING id, username")
        .bind(&body.username)
        .bind(body.email.as_ref().map(|e| normalize_email(e)))
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| {
            if let Some(db_err) = err.as_database_error()
                && let Some(api_err) = map_unique_violation_to_conflict(
                    db_err.code().as_deref(),
                    db_err.constraint(),
                )
            {
                return api_err;
            }
            ApiError::Internal(err.to_string())
        })?;

    tracing::info!(user_id = user_id, "User updated");

    Ok(Json(user))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    claims: Claims,
) -> Result<StatusCode, ApiError> {
    let auth_user_id = claims
        .sub
        .parse::<i64>()
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    if auth_user_id != user_id {
        return Err(ApiError::Forbidden);
    }

    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    if result.rows_affected() == 0 {
        Err(ApiError::NotFound)
    } else {
        tracing::info!(user_id = user_id, "User deleted");
        Ok(StatusCode::NO_CONTENT)
    }
}
