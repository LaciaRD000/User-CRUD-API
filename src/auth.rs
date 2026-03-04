use std::time::Duration;

use axum::extract::FromRequestParts;
use chrono::{DateTime, Utc};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation,
    errors::{ErrorKind, new_error},
};
use serde::{Deserialize, Serialize};

use crate::{errors::ApiError, state::AppState};

#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub iss: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
}

pub fn create_token(
    user_id: i64,
    secret: &str,
    issuer: &str,
    audience: &str,
    expiry_minutes: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let dt: DateTime<Utc> = Utc::now();

    let claims = Claims {
        sub: user_id.to_string(),
        iss: issuer.to_string(),
        aud: audience.to_string(),
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
    issuer: &str,
    audience: &str,
    leeway_seconds: u64,
) -> Result<Claims, jsonwebtoken::errors::Error> {
    // Accept only HS256 for token verification.
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = leeway_seconds;
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub", "iat"]);
    let claims = jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_ref()),
        &validation,
    )?;

    // Reject tokens with iat far in the future. Use the same leeway as exp
    // validation.
    let now = Utc::now().timestamp();
    let allowed_future = now.saturating_add(leeway_seconds as i64);
    let iat = i64::try_from(claims.claims.iat)
        .map_err(|_| new_error(ErrorKind::InvalidClaimFormat("iat".into())))?;
    if iat > allowed_future {
        return Err(new_error(ErrorKind::InvalidToken));
    }

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

        validate_token(
            token,
            &state.jwt_secret,
            &state.jwt_issuer,
            &state.jwt_audience,
            state.jwt_leeway_seconds,
        )
        .map_err(|err| {
            tracing::warn!("Token Validation failed: {}", err);
            ApiError::Unauthorized
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-at-least-32-chars!!";
    const TEST_ISSUER: &str = "test-issuer";
    const TEST_AUDIENCE: &str = "test-audience";
    const TEST_LEEWAY_SECONDS: u64 = 60;

    #[test]
    fn create_and_validate_token_roundtrip() {
        let user_id: i64 = 123456;
        let token =
            create_token(user_id, TEST_SECRET, TEST_ISSUER, TEST_AUDIENCE, 60)
                .unwrap();
        let claims = validate_token(
            &token,
            TEST_SECRET,
            TEST_ISSUER,
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        )
        .unwrap();
        assert_eq!(claims.sub, user_id.to_string());
    }

    #[test]
    fn validate_token_fails_with_wrong_secret() {
        let token =
            create_token(1, TEST_SECRET, TEST_ISSUER, TEST_AUDIENCE, 60)
                .unwrap();
        let result = validate_token(
            &token,
            "wrong-secret-key-at-least-32-chars!!",
            TEST_ISSUER,
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        );
        assert!(result.is_err());
    }

    #[test]
    fn validate_token_fails_when_expired() {
        // leeway (デフォルト60秒) を超えた過去の exp を直接設定
        let past = chrono::Utc::now().timestamp() as u64 - 120;
        let claims = Claims {
            sub: "1".to_string(),
            iss: TEST_ISSUER.to_string(),
            aud: TEST_AUDIENCE.to_string(),
            iat: past - 60,
            exp: past,
        };
        let token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_ref()),
        )
        .unwrap();
        let result = validate_token(
            &token,
            TEST_SECRET,
            TEST_ISSUER,
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        );
        assert!(result.is_err());
    }

    #[test]
    fn claims_sub_is_string_representation_of_user_id() {
        let user_id: i64 = 987654321;
        let token =
            create_token(user_id, TEST_SECRET, TEST_ISSUER, TEST_AUDIENCE, 60)
                .unwrap();
        let claims = validate_token(
            &token,
            TEST_SECRET,
            TEST_ISSUER,
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        )
        .unwrap();
        assert_eq!(claims.sub.parse::<i64>().unwrap(), user_id);
    }

    #[test]
    fn claims_iat_is_before_exp() {
        let token =
            create_token(1, TEST_SECRET, TEST_ISSUER, TEST_AUDIENCE, 60)
                .unwrap();
        let claims = validate_token(
            &token,
            TEST_SECRET,
            TEST_ISSUER,
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        )
        .unwrap();
        assert!(claims.iat < claims.exp);
    }

    #[test]
    fn validate_token_fails_with_garbage_input() {
        let result = validate_token(
            "not-a-jwt",
            TEST_SECRET,
            TEST_ISSUER,
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        );
        assert!(result.is_err());
    }

    #[test]
    fn validate_token_fails_with_wrong_issuer() {
        let token =
            create_token(1, TEST_SECRET, TEST_ISSUER, TEST_AUDIENCE, 60)
                .unwrap();
        let result = validate_token(
            &token,
            TEST_SECRET,
            "wrong-issuer",
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        );
        assert!(result.is_err());
    }

    #[test]
    fn validate_token_fails_with_wrong_audience() {
        let token =
            create_token(1, TEST_SECRET, TEST_ISSUER, TEST_AUDIENCE, 60)
                .unwrap();
        let result = validate_token(
            &token,
            TEST_SECRET,
            TEST_ISSUER,
            "wrong-audience",
            TEST_LEEWAY_SECONDS,
        );
        assert!(result.is_err());
    }

    #[test]
    fn validate_token_fails_when_iat_is_far_in_the_future() {
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = Claims {
            sub: "1".to_string(),
            iss: TEST_ISSUER.to_string(),
            aud: TEST_AUDIENCE.to_string(),
            iat: now + 5,
            exp: now + 60,
        };
        let token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_ref()),
        )
        .unwrap();

        let result = validate_token(
            &token,
            TEST_SECRET,
            TEST_ISSUER,
            TEST_AUDIENCE,
            0, // no leeway
        );
        assert!(result.is_err());
    }

    #[test]
    fn validate_token_allows_small_future_iat_within_leeway() {
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = Claims {
            sub: "1".to_string(),
            iss: TEST_ISSUER.to_string(),
            aud: TEST_AUDIENCE.to_string(),
            iat: now + 30,
            exp: now + 60,
        };
        let token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_ref()),
        )
        .unwrap();

        let claims =
            validate_token(&token, TEST_SECRET, TEST_ISSUER, TEST_AUDIENCE, 60)
                .unwrap();
        assert_eq!(claims.sub, "1");
    }

    #[test]
    fn validate_token_fails_when_iat_is_too_large_to_represent_as_i64() {
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = Claims {
            sub: "1".to_string(),
            iss: TEST_ISSUER.to_string(),
            aud: TEST_AUDIENCE.to_string(),
            iat: u64::MAX,
            exp: now + 60,
        };
        let token = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET.as_ref()),
        )
        .unwrap();

        let result = validate_token(
            &token,
            TEST_SECRET,
            TEST_ISSUER,
            TEST_AUDIENCE,
            TEST_LEEWAY_SECONDS,
        );
        assert!(result.is_err());
    }
}
