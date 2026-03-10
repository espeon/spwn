mod password;
pub mod routes;
mod session;

pub use routes::auth_router;
pub use session::AccountId;

pub fn validate_username(username: &str) -> Result<(), &'static str> {
    if username.len() < 3 {
        return Err("username must be at least 3 characters");
    }
    if username.len() > 32 {
        return Err("username must be at most 32 characters");
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("username may only contain letters, numbers, and hyphens");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_username;

    #[test]
    fn test_valid_username() {
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("alice-bob").is_ok());
        assert!(validate_username("abc").is_ok());
        assert!(validate_username("a1b2c3").is_ok());
        assert!(validate_username(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn test_username_too_short() {
        let err = validate_username("ab").unwrap_err();
        assert!(err.contains("3 characters"));
    }

    #[test]
    fn test_username_too_long() {
        let err = validate_username(&"a".repeat(33)).unwrap_err();
        assert!(err.contains("32 characters"));
    }

    #[test]
    fn test_username_invalid_chars() {
        for bad in &["bad_name", "bad name", "bad.name", "bad!name", "bad@name"] {
            assert!(validate_username(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn test_username_hyphens_allowed() {
        assert!(validate_username("my-cool-username").is_ok());
    }
}
