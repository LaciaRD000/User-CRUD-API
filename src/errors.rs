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
                (StatusCode::NOT_FOUND, Json(json!({"error": "NotFound"})))
                    .into_response()
            }
            ApiError::Internal(s) => {
                tracing::error!("Internal error: {s}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "An internal error occurred"})),
                )
                    .into_response()
            }
            ApiError::BadRequest(s) => {
                (StatusCode::BAD_REQUEST, Json(json!({"error": s})))
                    .into_response()
            }
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            )
                .into_response(),
            ApiError::Forbidden => {
                (StatusCode::FORBIDDEN, Json(json!({"error": "Forbidden"})))
                    .into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn status_and_body(error: ApiError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn not_found_returns_404() {
        let (status, body) = status_and_body(ApiError::NotFound).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "NotFound");
    }

    #[tokio::test]
    async fn bad_request_returns_400_with_message() {
        let (status, body) = status_and_body(ApiError::BadRequest("invalid input".into())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "invalid input");
    }

    #[tokio::test]
    async fn internal_returns_500_without_leaking_details() {
        let (status, body) = status_and_body(ApiError::Internal("db crash".into())).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"], "An internal error occurred");
        assert!(!body["error"].as_str().unwrap().contains("db crash"));
    }

    #[tokio::test]
    async fn unauthorized_returns_401() {
        let (status, body) = status_and_body(ApiError::Unauthorized).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "Unauthorized");
    }

    #[tokio::test]
    async fn forbidden_returns_403() {
        let (status, body) = status_and_body(ApiError::Forbidden).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "Forbidden");
    }
}
