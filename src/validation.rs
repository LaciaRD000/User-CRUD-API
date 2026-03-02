pub fn validate_username(username: &str) -> Result<(), String> {
    if username.is_empty() {
        Err("username is empty".to_string())
    } else if username.len() > 32 {
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
