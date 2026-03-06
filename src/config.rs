use crate::rate_limit::RateLimitIpMode;

pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub jwt_leeway_seconds: u64,
    pub access_token_expiry_minutes: u64,
    pub refresh_token_expiry_days: u64,
    pub refresh_token_pepper: String,
    pub rate_limit_ip_mode: RateLimitIpMode,
    pub snowflake_machine_id: u16,
}

impl Config {
    pub fn from_env() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

        let jwt_secret =
            std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
        if jwt_secret.len() < 32 {
            panic!("JWT_SECRET must be at least 32 characters");
        }

        let jwt_issuer =
            std::env::var("JWT_ISSUER").expect("JWT_ISSUER must be set");
        let jwt_audience =
            std::env::var("JWT_AUDIENCE").expect("JWT_AUDIENCE must be set");

        let jwt_leeway_seconds = std::env::var("JWT_LEEWAY_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);

        let access_token_expiry_minutes =
            std::env::var("ACCESS_TOKEN_EXPIRY_MINUTES")
                .expect("ACCESS_TOKEN_EXPIRY_MINUTES must be set")
                .parse::<u64>()
                .expect("ACCESS_TOKEN_EXPIRY_MINUTES could not parse u64");

        let refresh_token_expiry_days =
            std::env::var("REFRESH_TOKEN_EXPIRY_DAYS")
                .expect("REFRESH_TOKEN_EXPIRY_DAYS must be set")
                .parse::<u64>()
                .expect("REFRESH_TOKEN_EXPIRY_DAYS could not parse u64");

        let refresh_token_pepper = std::env::var("REFRESH_TOKEN_PEPPER")
            .expect("REFRESH_TOKEN_PEPPER must be set");

        let rate_limit_ip_mode = std::env::var("RATE_LIMIT_IP_MODE")
            .ok()
            .map(|s| {
                RateLimitIpMode::parse(&s).unwrap_or_else(|| {
                    panic!(
                        "RATE_LIMIT_IP_MODE must be one of: peer, smart (got: {s})"
                    )
                })
            })
            .unwrap_or(RateLimitIpMode::Peer);

        let snowflake_machine_id = std::env::var("SNOWFLAKE_MACHINE_ID")
            .expect("SNOWFLAKE_MACHINE_ID must be set")
            .parse::<u16>()
            .expect("SNOWFLAKE_MACHINE_ID could not parse u16");

        Self {
            database_url,
            jwt_secret,
            jwt_issuer,
            jwt_audience,
            jwt_leeway_seconds,
            access_token_expiry_minutes,
            refresh_token_expiry_days,
            refresh_token_pepper,
            rate_limit_ip_mode,
            snowflake_machine_id,
        }
    }
}
