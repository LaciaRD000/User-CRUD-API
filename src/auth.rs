use std::time::Duration;

use axum::extract::FromRequestParts;
use chrono::{DateTime, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::{errors::ApiError, state::AppState};

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub exp: u64,
    pub iat: u64,
}

pub fn create_token(
    user_id: i64,
    secret: &str,
    expiry_minutes: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let dt: DateTime<Utc> = Utc::now();

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

pub fn validate_token(
    token: &str,
    secret: &str,
) -> Result<Claims, jsonwebtoken::errors::Error> {
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

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-at-least-32-chars!!";

    #[test]
    fn create_and_validate_token_roundtrip() {
        let user_id: i64 = 123456;
        let token = create_token(user_id, TEST_SECRET, 60).unwrap();
        let claims = validate_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub, user_id.to_string());
    }

    #[test]
    fn validate_token_fails_with_wrong_secret() {
        let token = create_token(1, TEST_SECRET, 60).unwrap();
        let result = validate_token(&token, "wrong-secret-key-at-least-32-chars!!");
        assert!(result.is_err());
    }

    #[test]
    fn validate_token_fails_when_expired() {
        // leeway (デフォルト60秒) を超えた過去の exp を直接設定
        let past = chrono::Utc::now().timestamp() as u64 - 120;
        let claims = Claims {
            sub: "1".to_string(),
            iat: past - 60,
            exp: past,
        };
        let token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_ref()),
        )
        .unwrap();
        let result = validate_token(&token, TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn claims_sub_is_string_representation_of_user_id() {
        let user_id: i64 = 987654321;
        let token = create_token(user_id, TEST_SECRET, 60).unwrap();
        let claims = validate_token(&token, TEST_SECRET).unwrap();
        assert_eq!(claims.sub.parse::<i64>().unwrap(), user_id);
    }

    #[test]
    fn claims_iat_is_before_exp() {
        let token = create_token(1, TEST_SECRET, 60).unwrap();
        let claims = validate_token(&token, TEST_SECRET).unwrap();
        assert!(claims.iat < claims.exp);
    }

    #[test]
    fn validate_token_fails_with_garbage_input() {
        let result = validate_token("not-a-jwt", TEST_SECRET);
        assert!(result.is_err());
    }
}
