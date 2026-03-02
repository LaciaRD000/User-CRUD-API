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
