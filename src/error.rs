use crate::api::client::ApiError;
use crate::auth::AuthError;

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
}

impl CliError {
    pub fn to_exit_code(&self) -> i32 {
        match self {
            Self::Auth(AuthError::NotLoggedIn) => 10,
            Self::Auth(AuthError::BlockedByCa) | Self::Auth(AuthError::Aad { .. }) => 11,
            Self::Api(ApiError::NotFound(_)) => 20,
            Self::Api(ApiError::RateLimited) => 30,
            Self::Api(ApiError::Transport(_)) => 40,
            Self::Http(_) | Self::Io(_) => 40,
            _ => 1,
        }
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
}
