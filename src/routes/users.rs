use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    errors::ApiError,
    models::{UpdateUser, User},
    state::AppState,
    validation::{validate_email, validate_username},
};

pub async fn list_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<User>>, ApiError> {
    let users: Vec<User> = sqlx::query_as("SELECT * FROM users ORDER BY id")
        .fetch_all(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;
    Ok(Json(users))
}

pub async fn get_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<User>, ApiError> {
    let user: Option<User> =
        sqlx::query_as("SELECT * FROM users WHERE id = $1")
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
    body: Json<UpdateUser>,
) -> Result<Json<User>, ApiError> {
    if let Some(ref username) = body.username {
        validate_username(username).map_err(|err| ApiError::BadRequest(err))?;
    }

    if let Some(ref email) = body.email {
        validate_email(email).map_err(|err| ApiError::BadRequest(err))?;
    }

    let user: User = sqlx::query_as("UPDATE users SET username = COALESCE($1, username), email = COALESCE($2, email) where id = $3 RETURNING *")
        .bind(&body.username)
        .bind(&body.email)
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok(Json(user))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
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
