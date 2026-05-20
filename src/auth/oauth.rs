use super::{AuthError, CLIENT_ID, SCOPE};
use serde::Deserialize;
use tokio::time::{sleep, Duration, Instant};

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    #[serde(alias = "verification_url")]
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_in: i64,
    #[allow(dead_code)]
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AadErrorResponse {
    error: String,
    #[serde(default)]
    error_description: String,
}

pub async fn request_device_code(
    http: &reqwest::Client,
    tenant: &str,
) -> Result<DeviceCodeResponse, AuthError> {
    let url = format!("{}/{tenant}/oauth2/v2.0/devicecode", auth_base());
    let response = http
        .post(url)
        .form(&[("client_id", CLIENT_ID), ("scope", SCOPE)])
        .send()
        .await?;
    parse_aad_response(response).await
}

pub async fn poll_for_token(
    http: &reqwest::Client,
    tenant: &str,
    dc: &DeviceCodeResponse,
) -> Result<TokenResponse, AuthError> {
    let url = format!("{}/{tenant}/oauth2/v2.0/token", auth_base());
    let started = Instant::now();
    let expires = Duration::from_secs(dc.expires_in);
    let mut interval = dc.interval.max(1);

    while started.elapsed() < expires {
        tracing::info!(
            "polling device-code token endpoint ({}s interval)",
            interval
        );
        sleep(Duration::from_secs(interval)).await;
        let response = http
            .post(&url)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", CLIENT_ID),
                ("device_code", dc.device_code.as_str()),
            ])
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(response.json().await?);
        }

        let status = response.status();
        let error = response
            .json::<AadErrorResponse>()
            .await
            .map_err(AuthError::Http)?;
        match error.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += 5,
            "authorization_declined" | "expired_token" => {
                return Err(AuthError::DeviceCode(error.error_description));
            }
            "access_denied" if is_ca_block(&error.error_description) => {
                return Err(AuthError::BlockedByCa);
            }
            _ if is_ca_block(&error.error_description) => return Err(AuthError::BlockedByCa),
            _ => {
                return Err(AuthError::Aad {
                    code: error.error,
                    msg: format!("{} (HTTP {status})", error.error_description),
                });
            }
        }
    }

    Err(AuthError::DeviceCode("device code expired".to_string()))
}

pub async fn refresh_access_token(
    http: &reqwest::Client,
    tenant: &str,
    refresh_token: &str,
    scope: &str,
) -> Result<TokenResponse, AuthError> {
    let url = format!("{}/{tenant}/oauth2/v2.0/token", auth_base());
    let response = http
        .post(url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
            ("scope", scope),
        ])
        .send()
        .await?;
    parse_aad_response(response).await
}

async fn parse_aad_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, AuthError> {
    if response.status().is_success() {
        return Ok(response.json().await?);
    }
    let status = response.status();
    let text = response.text().await?;
    if is_ca_block(&text) {
        return Err(AuthError::BlockedByCa);
    }
    if let Ok(error) = serde_json::from_str::<AadErrorResponse>(&text) {
        return Err(AuthError::Aad {
            code: error.error,
            msg: format!("{} (HTTP {status})", error.error_description),
        });
    }
    Err(AuthError::Aad {
        code: status.as_u16().to_string(),
        msg: "AAD request failed".to_string(),
    })
}

fn auth_base() -> String {
    std::env::var("TEAMS_AUTH_BASE")
        .unwrap_or_else(|_| "https://login.microsoftonline.com".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn is_ca_block(text: &str) -> bool {
    text.contains("AADSTS50059") || text.contains("AADSTS530032")
}
