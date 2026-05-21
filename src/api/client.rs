use crate::auth::{AuthError, Session, USER_AGENT, X_MS_CLIENT_VERSION};
use reqwest::Method;
use serde::de::DeserializeOwned;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    session: Arc<Mutex<Session>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum AuthStyle {
    SkypeHeader,
    BearerSkype,
    BearerAad,
    BearerAadPlusSkype,
}

#[derive(thiserror::Error, Debug)]
pub enum ApiError {
    #[error("auth: {0}")]
    Auth(#[from] AuthError),
    #[error("http {status}")]
    Http { status: u16, body: String },
    #[error("rate limited after retries")]
    RateLimited,
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("not found")]
    NotFound(String),
}

impl ApiClient {
    pub fn new(session: Session) -> Result<Self, ApiError> {
        let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self {
            http,
            session: Arc::new(Mutex::new(session)),
        })
    }

    pub async fn chat_service(&self) -> String {
        self.region_value("chatService", "https://amer.ng.msg.teams.microsoft.com")
            .await
    }

    pub async fn chat_svc_agg(&self) -> String {
        self.region_value(
            "chatServiceAggregator",
            "https://chatsvcagg.teams.microsoft.com",
        )
        .await
    }

    pub async fn middle_tier(&self) -> String {
        self.region_value("middleTier", "https://teams.microsoft.com/api/mt/amer")
            .await
    }

    pub async fn csa_base(&self) -> String {
        if let Some(region) = self.middle_tier().await.rsplit('/').next() {
            format!("https://teams.microsoft.com/api/csa/{region}")
        } else {
            "https://teams.microsoft.com/api/csa/amer".to_string()
        }
    }

    pub async fn display_name(&self) -> String {
        self.session
            .lock()
            .await
            .state
            .identity
            .display_name
            .clone()
    }

    pub async fn user_oid(&self) -> String {
        self.session.lock().await.state.identity.user_oid.clone()
    }

    pub async fn tenant_id(&self) -> String {
        self.session.lock().await.state.identity.tenant_id.clone()
    }

    pub async fn upn(&self) -> String {
        self.session.lock().await.state.identity.upn.clone()
    }

    pub async fn request(
        &self,
        method: Method,
        url: &str,
        style: AuthStyle,
    ) -> Result<reqwest::RequestBuilder, ApiError> {
        let mut session = self.session.lock().await;
        session.ensure_valid(&self.http).await?;
        let mut builder = self
            .http
            .request(method, url)
            .header("x-ms-client-version", X_MS_CLIENT_VERSION)
            .header("x-ms-client-type", "web")
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::ACCEPT_LANGUAGE, "ko-kr");
        match style {
            AuthStyle::SkypeHeader => {
                builder = builder.header(
                    "Authentication",
                    format!("skypetoken={}", session.skype.as_str()),
                );
            }
            AuthStyle::BearerSkype => {
                builder = builder.bearer_auth(session.skype.as_str());
            }
            AuthStyle::BearerAad => {
                builder = builder.bearer_auth(session.aad_access.as_str());
            }
            AuthStyle::BearerAadPlusSkype => {
                builder = builder.bearer_auth(session.aad_access.as_str()).header(
                    "Authentication",
                    format!("skypetoken={}", session.skype.as_str()),
                );
            }
        }
        Ok(builder)
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        style: AuthStyle,
    ) -> Result<T, ApiError> {
        with_retries(|| async {
            let response = self.request(Method::GET, url, style).await?.send().await?;
            parse_response(response).await
        })
        .await
    }

    pub async fn post_json<T: DeserializeOwned, B: serde::Serialize + ?Sized>(
        &self,
        url: &str,
        style: AuthStyle,
        body: &B,
    ) -> Result<T, ApiError> {
        with_retries(|| async {
            let response = self
                .request(Method::POST, url, style)
                .await?
                .json(body)
                .send()
                .await?;
            parse_response(response).await
        })
        .await
    }

    async fn region_value(&self, key: &str, fallback: &str) -> String {
        self.session
            .lock()
            .await
            .state
            .region_gtms
            .get(key)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    }
}

async fn parse_response<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ApiError> {
    let status = response.status();
    if status.is_success() {
        return response.json::<T>().await.map_err(ApiError::Transport);
    }
    let body = response.text().await.unwrap_or_default();
    if status.as_u16() == 404 {
        return Err(ApiError::NotFound(body));
    }
    if status.as_u16() == 429 {
        return Err(ApiError::RateLimited);
    }
    Err(ApiError::Http {
        status: status.as_u16(),
        body,
    })
}

pub async fn with_retries<F, Fut, T>(operation: F) -> Result<T, ApiError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, ApiError>>,
{
    let mut delay = Duration::from_millis(250);
    for attempt in 0..4 {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(ApiError::RateLimited) if attempt < 3 => {
                sleep(delay).await;
                delay *= 2;
            }
            Err(ApiError::Http { status, .. }) if status >= 500 && attempt < 3 => {
                sleep(delay).await;
                delay *= 2;
            }
            Err(error) => return Err(error),
        }
    }
    Err(ApiError::RateLimited)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::store::SecretString;
    use crate::config::{Expiry, Identity, State};
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn builds_csa_base_from_middle_tier_region() {
        let mut region_gtms = BTreeMap::new();
        region_gtms.insert(
            "middleTier".to_string(),
            "https://teams.microsoft.com/api/mt/apac".to_string(),
        );
        let session = Session {
            aad_access: SecretString::new("aad".into()),
            aad_refresh: SecretString::new("refresh".into()),
            aad_id: SecretString::new("id".into()),
            skype: SecretString::new("skype".into()),
            state: State {
                schema_version: 1,
                identity: Identity {
                    tenant_id: "tenant".into(),
                    user_oid: "oid".into(),
                    display_name: "name".into(),
                    upn: "u@example.com".into(),
                },
                expiry: Expiry {
                    aad_access_exp: 4_102_444_800,
                    skype_exp: 4_102_444_800,
                },
                region_gtms,
            },
        };
        let client = ApiClient::new(session).expect("client");
        assert_eq!(
            client.csa_base().await,
            "https://teams.microsoft.com/api/csa/apac"
        );
    }
}
