use super::{AuthError, TOKEN_EARLY_EXPIRY_SECS};
use base64::Engine;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct AadClaims {
    pub oid: String,
    pub tid: String,
    pub name: Option<String>,
    pub upn: Option<String>,
    pub preferred_username: Option<String>,
    pub exp: i64,
}

pub fn parse_claims(token: &str) -> Result<AadClaims, AuthError> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| AuthError::Parse("JWT is missing a payload segment".to_string()))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| AuthError::Parse(format!("invalid JWT payload base64: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| AuthError::Parse(format!("invalid JWT payload json: {error}")))
}

pub fn is_expired(exp: i64) -> bool {
    chrono::Utc::now().timestamp() + TOKEN_EARLY_EXPIRY_SECS >= exp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claims_from_unsigned_test_token() {
        let payload = r#"{"oid":"o","tid":"t","name":"n","upn":"u@example.com","exp":4102444800}"#;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        let token = format!("header.{encoded}.sig");
        let claims = parse_claims(&token).expect("claims");
        assert_eq!(claims.oid, "o");
        assert_eq!(claims.tid, "t");
        assert_eq!(claims.upn.as_deref(), Some("u@example.com"));
    }
}
