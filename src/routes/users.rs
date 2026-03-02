use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    errors::ApiError,
    models::{CreateUser, UpdateUser, User},
    state::AppState,
};

use crate::validation::{validate_email, validate_username};

pub async fn create_user(
    State(state): State<AppState>,
    body: Json<CreateUser>,
) -> Result<(StatusCode, Json<User>), ApiError> {
    validate_username(&body.username).map_err(|err| ApiError::BadRequest(err))?;
    validate_email(&body.email).map_err(|err| ApiError::BadRequest(err))?;

    let snowflake = state.snowflake.lock().unwrap().generate();

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, username, email) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(snowflake)
    .bind(&body.username)
    .bind(&body.email)
    .fetch_one(&state.db)
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?;

    Ok((StatusCode::CREATED, Json(user)))
}

pub async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<User>>, ApiError> {
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
}

pub async fn update_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    body: Json<UpdateUser>,
) -> Result<Json<User>, ApiError> {
}

pub async fn delete_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
}
