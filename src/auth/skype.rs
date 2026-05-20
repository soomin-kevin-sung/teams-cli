use super::{AuthError, AUTHSVC_URL, USER_AGENT, X_MS_CLIENT_VERSION};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AuthzResponse {
    pub tokens: AuthzTokens,
    #[serde(rename = "regionGtms")]
    pub region_gtms: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct AuthzTokens {
    #[serde(rename = "skypeToken")]
    pub skype_token: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: i64,
}

pub async fn exchange_skype_token(
    http: &reqwest::Client,
    aad_access: &str,
) -> Result<AuthzResponse, AuthError> {
    let url = std::env::var("TEAMS_AUTHSVC_URL").unwrap_or_else(|_| AUTHSVC_URL.to_string());
    let response = http
        .post(url)
        .bearer_auth(aad_access)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header("x-ms-client-version", X_MS_CLIENT_VERSION)
        .header("x-ms-client-type", "web")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .body(Vec::new())
        .send()
        .await?;
    if response.status().is_success() {
        return Ok(response.json().await?);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(AuthError::Aad {
        code: status.as_u16().to_string(),
        msg: format!("authsvc token exchange failed: {body}"),
    })
}
