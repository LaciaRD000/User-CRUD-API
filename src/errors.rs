use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub enum ApiError {
    NotFound,
    BadRequest(String),
    Internal(String),
    Unauthorized,
    Forbidden,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::NotFound => {
                (StatusCode::NOT_FOUND, Json(json!({"error": "NotFound"}))).into_response()
            }
            ApiError::Internal(s) => {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": s}))).into_response()
            }
            ApiError::BadRequest(s) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": s}))).into_response()
            }
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            )
                .into_response(),
            ApiError::Forbidden => {
                (StatusCode::FORBIDDEN, Json(json!({"error": "Forbidden"}))).into_response()
            }
        }
    }
}
