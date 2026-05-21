use crate::api::client::ApiError;
use crate::auth::AuthError;
use serde_json::{json, Value};

#[derive(thiserror::Error, Debug)]
pub enum CliError {
    #[error("{0}")]
    Auth(#[from] AuthError),
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("{0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("{0}")]
    Other(String),
    #[error("{message}")]
    Structured {
        code: &'static str,
        message: String,
        details: Value,
        exit_code: i32,
    },
}

impl CliError {
    pub fn structured(
        code: &'static str,
        message: impl Into<String>,
        details: Value,
        exit_code: i32,
    ) -> Self {
        Self::Structured {
            code,
            message: message.into(),
            details,
            exit_code,
        }
    }

    pub fn to_exit_code(&self) -> i32 {
        match self {
            Self::Structured { exit_code, .. } => *exit_code,
            Self::Auth(AuthError::NotLoggedIn) => 10,
            Self::Auth(AuthError::BlockedByCa) | Self::Auth(AuthError::Aad { .. }) => 11,
            Self::Api(ApiError::NotFound(_)) => 20,
            Self::Api(ApiError::RateLimited) => 30,
            Self::Api(ApiError::Transport(_)) => 40,
            Self::Http(_) | Self::Io(_) => 40,
            _ => 1,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Structured { code, .. } => code,
            Self::Auth(AuthError::NotLoggedIn) => "not_logged_in",
            Self::Auth(AuthError::BlockedByCa) => "conditional_access_blocked",
            Self::Auth(AuthError::Aad { .. }) => "aad_error",
            Self::Auth(_) => "auth_error",
            Self::Api(ApiError::Auth(_)) => "auth_error",
            Self::Api(ApiError::Http { .. }) => "http_error",
            Self::Api(ApiError::NotFound(_)) => "not_found",
            Self::Api(ApiError::RateLimited) => "rate_limited",
            Self::Api(ApiError::Transport(_)) => "transport_error",
            Self::Api(ApiError::Decode(_)) => "decode_error",
            Self::Io(_) => "io_error",
            Self::Http(_) => "transport_error",
            Self::Json(_) => "json_error",
            Self::TomlSer(_) | Self::TomlDe(_) => "toml_error",
            Self::Other(_) => "error",
        }
    }

    pub fn details(&self) -> Value {
        match self {
            Self::Structured { details, .. } => details.clone(),
            Self::Auth(AuthError::Aad { code, msg }) => {
                json!({ "aad_code": code, "aad_message": msg })
            }
            Self::Api(ApiError::Http { status, .. }) => json!({ "status": status }),
            _ => json!({}),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": false,
            "error": {
                "code": self.code(),
                "message": self.to_string(),
                "exit_code": self.to_exit_code(),
                "details": self.details()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_logged_in_exit_code_is_ten() {
        let error = CliError::Auth(AuthError::NotLoggedIn);
        assert_eq!(error.to_exit_code(), 10);
    }

    #[test]
    fn structured_error_has_machine_code_and_details() {
        let error = CliError::structured(
            "ambiguous_target",
            "ambiguous",
            json!({ "candidates": [1, 2] }),
            2,
        );

        assert_eq!(error.to_exit_code(), 2);
        assert_eq!(error.code(), "ambiguous_target");
        assert_eq!(error.to_json()["error"]["details"]["candidates"][0], 1);
    }
}
