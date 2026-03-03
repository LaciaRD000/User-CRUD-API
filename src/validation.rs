pub fn validate_username(username: &str) -> Result<(), String> {
    if username.is_empty() {
        Err("username is empty".to_string())
    } else if username.chars().count() > 32 {
        Err("username is too long".to_string())
    } else {
        Ok(())
    }
}

pub fn validate_email(email: &str) -> Result<(), String> {
    if email.is_empty() {
        Err("email is empty".to_string())
    } else if !email.contains("@") {
        Err("email is invalid".to_string())
    } else {
        Ok(())
    }
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub fn validate_password(password: &str) -> Result<(), String> {
    if password.is_empty() {
        Err("password is empty".to_string())
    } else if password.chars().count() < 8 {
        Err("password is too short".to_string())
    } else if password.len() > 72 {
        Err("password is too long".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_username ---

    #[test]
    fn username_valid() {
        assert!(validate_username("taro").is_ok());
    }

    #[test]
    fn username_single_char_is_valid() {
        assert!(validate_username("a").is_ok());
    }

    #[test]
    fn username_32_chars_is_valid() {
        let name = "a".repeat(32);
        assert!(validate_username(&name).is_ok());
    }

    #[test]
    fn username_empty_is_rejected() {
        let err = validate_username("").unwrap_err();
        assert!(err.contains("empty"), "Expected 'empty' in error: {err}");
    }

    #[test]
    fn username_33_chars_is_rejected() {
        let name = "a".repeat(33);
        let err = validate_username(&name).unwrap_err();
        assert!(err.contains("long"), "Expected 'long' in error: {err}");
    }

    // --- validate_email ---

    #[test]
    fn email_valid() {
        assert!(validate_email("user@example.com").is_ok());
    }

    #[test]
    fn email_empty_is_rejected() {
        let err = validate_email("").unwrap_err();
        assert!(err.contains("empty"), "Expected 'empty' in error: {err}");
    }

    #[test]
    fn email_without_at_is_rejected() {
        let err = validate_email("invalid-email").unwrap_err();
        assert!(
            err.contains("invalid"),
            "Expected 'invalid' in error: {err}"
        );
    }

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email("  User@Example.COM  "),
            "user@example.com"
        );
    }

    // --- validate_password ---

    #[test]
    fn password_valid() {
        assert!(validate_password("password123").is_ok());
    }

    #[test]
    fn password_8_chars_is_valid() {
        assert!(validate_password("12345678").is_ok());
    }

    #[test]
    fn password_empty_is_rejected() {
        let err = validate_password("").unwrap_err();
        assert!(err.contains("empty"), "Expected 'empty' in error: {err}");
    }

    #[test]
    fn password_7_chars_is_rejected() {
        let err = validate_password("1234567").unwrap_err();
        assert!(err.contains("short"), "Expected 'short' in error: {err}");
    }

    #[test]
    fn password_73_bytes_is_rejected() {
        let long = "a".repeat(73);
        let err = validate_password(&long).unwrap_err();
        assert!(err.contains("long"), "Expected 'long' in error: {err}");
    }

    #[test]
    fn password_72_bytes_is_valid() {
        let pw = "a".repeat(72);
        assert!(validate_password(&pw).is_ok());
    }
}
