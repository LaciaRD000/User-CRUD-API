use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    auth::Claims,
    errors::ApiError,
    models::{UpdateUser, User},
    state::AppState,
    validation::{normalize_email, validate_email, validate_username},
};

#[derive(Deserialize)]
pub struct UsersPagination {
    pub limit: Option<i64>,
    pub after_id: Option<i64>,
}

pub async fn list_users(
    State(state): State<AppState>,
    Query(pagination): Query<UsersPagination>,
) -> Result<Json<Vec<User>>, ApiError> {
    let limit = pagination.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::BadRequest("limit is out of range".into()));
    }

    let users: Vec<User> = match pagination.after_id {
        Some(after_id) => {
            if after_id < 0 {
                return Err(ApiError::BadRequest(
                    "after_id must be >= 0".into(),
                ));
            }
            sqlx::query_as(
                "SELECT id, username, email FROM users WHERE id > $1 ORDER BY id LIMIT $2",
            )
            .bind(after_id)
            .bind(limit)
            .fetch_all(&state.db)
            .await
            .map_err(|err| ApiError::Internal(err.to_string()))?
        }
        None => sqlx::query_as(
            "SELECT id, username, email FROM users ORDER BY id LIMIT $1",
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
) -> Result<Json<User>, ApiError> {
    let user: Option<User> =
        sqlx::query_as("SELECT id, username, email FROM users WHERE id = $1")
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
) -> Result<Json<User>, ApiError> {
    let auth_user_id = claims
        .sub
        .parse::<i64>()
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    if auth_user_id != user_id {
        return Err(ApiError::Forbidden);
    }

    if let Some(ref username) = body.username {
        validate_username(username).map_err(|err| ApiError::BadRequest(err))?;
    }

    if let Some(ref email) = body.email {
        let email = normalize_email(email);
        validate_email(&email).map_err(|err| ApiError::BadRequest(err))?;
    }

    let user: User = sqlx::query_as("UPDATE users SET username = COALESCE($1, username), email = COALESCE($2, email) where id = $3 RETURNING id, username, email")
        .bind(&body.username)
        .bind(body.email.as_ref().map(|e| normalize_email(e)))
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

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
        Ok(StatusCode::NO_CONTENT)
    }
}
