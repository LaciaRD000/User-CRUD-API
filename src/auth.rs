use std::time::Duration;

use crate::{errors::ApiError, state::AppState};
use axum::extract::FromRequestParts;
use chrono::{DateTime, Local};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub exp: u64,
    pub iat: u64,
    // pub user_name: String
    // pub iss: String
}

pub fn create_token(
    user_id: i64,
    secret: &str,
    expiry_minutes: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let dt: DateTime<Local> = Local::now();

    let claims = Claims {
        sub: user_id.to_string(),
        iat: dt.timestamp() as u64,
        exp: (dt + Duration::from_mins(expiry_minutes)).timestamp() as u64,
    };
    jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
}

pub fn validate_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let claims = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &Validation::default(),
    )?;
    Ok(claims.claims)
}

impl FromRequestParts<AppState> for Claims {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("Authorization")
            .ok_or(ApiError::Unauthorized)?;
        let auth_str = header.to_str().map_err(|_| ApiError::Unauthorized)?;
        let token = auth_str
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized)?;

        validate_token(token, &state.jwt_secret).map_err(|err| {
            tracing::warn!("Token Validation failed: {}", err);
            ApiError::Unauthorized
        })
    }
}
