pub mod jwt;
pub mod oauth;
pub mod skype;
pub mod store;

use crate::config::{AppPaths, State};
use jwt::{is_expired, parse_claims};
use serde_json::Value;
use std::collections::BTreeMap;
use store::SecretString;

pub const CLIENT_ID: &str = "1fec8e78-bce4-4aaf-ab1b-5451cc387264";
pub const SCOPE: &str = "https://api.spaces.skype.com/.default offline_access openid profile";
pub const AUTHSVC_URL: &str = "https://teams.microsoft.com/api/authsvc/v1.0/authz";
pub const X_MS_CLIENT_VERSION: &str = "1415/26041617215";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Teams/2.0.0.0 Chrome/124.0.0.0 Electron/30.0.0 Safari/537.36";
pub const TOKEN_EARLY_EXPIRY_SECS: i64 = 300;

#[derive(Debug)]
pub struct Session {
    pub aad_access: SecretString,
    pub aad_refresh: SecretString,
    pub aad_id: SecretString,
    pub skype: SecretString,
    pub state: State,
}

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("not logged in (run `teams login`)")]
    NotLoggedIn,
    #[error("device-code flow declined or timed out: {0}")]
    DeviceCode(String),
    #[error("AAD error {code}: {msg}")]
    Aad { code: String, msg: String },
    #[error("Conditional Access blocks device code (AADSTS50059/530032)")]
    BlockedByCa,
    #[error("keyring: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("config: {0}")]
    Config(String),
}

impl Session {
    pub async fn load(http: &reqwest::Client) -> Result<Self, AuthError> {
        let paths = AppPaths::resolve()?;
        let state = State::load(&paths.state)
            .map_err(|error| AuthError::Config(error.to_string()))?
            .ok_or(AuthError::NotLoggedIn)?;
        let store = store::default_store(&paths);
        let aad_access = store.get("aad_access")?.ok_or(AuthError::NotLoggedIn)?;
        let aad_refresh = store.get("aad_refresh")?.ok_or(AuthError::NotLoggedIn)?;
        let aad_id = store.get("aad_id")?.ok_or(AuthError::NotLoggedIn)?;
        let skype = store.get("skype")?.ok_or(AuthError::NotLoggedIn)?;
        let mut session = Self {
            aad_access,
            aad_refresh,
            aad_id,
            skype,
            state,
        };
        session.ensure_valid(http).await?;
        Ok(session)
    }

    pub async fn login_interactive(
        http: &reqwest::Client,
        tenant: &str,
    ) -> Result<Self, AuthError> {
        let device_code = oauth::request_device_code(http, tenant).await?;
        if let Some(message) = &device_code.message {
            println!("{message}");
        } else {
            println!(
                "To sign in, open {} and enter the code: {}",
                device_code.verification_uri, device_code.user_code
            );
        }
        println!(
            "Waiting for sign-in... (expires in {}m)",
            device_code.expires_in / 60
        );

        let token = oauth::poll_for_token(http, tenant, &device_code).await?;
        let claims = parse_claims(&token.id_token)?;
        let display_name = claims.name.unwrap_or_else(|| "Unknown".to_string());
        let upn = claims
            .upn
            .or(claims.preferred_username)
            .unwrap_or_else(|| "unknown".to_string());
        let mut state = State::empty_from_claims(
            claims.tid,
            claims.oid,
            display_name,
            upn,
            claims
                .exp
                .min(chrono::Utc::now().timestamp() + token.expires_in),
        );
        let authz = skype::exchange_skype_token(http, &token.access_token).await?;
        state.expiry.skype_exp = chrono::Utc::now().timestamp() + authz.tokens.expires_in;
        state.region_gtms = value_to_string_map(authz.region_gtms);

        let session = Self {
            aad_access: SecretString::new(token.access_token),
            aad_refresh: SecretString::new(token.refresh_token),
            aad_id: SecretString::new(token.id_token),
            skype: SecretString::new(authz.tokens.skype_token),
            state,
        };
        session.persist()?;
        Ok(session)
    }

    pub async fn ensure_valid(&mut self, http: &reqwest::Client) -> Result<(), AuthError> {
        let tenant = self.state.identity.tenant_id.clone();
        let mut changed = false;
        if is_expired(self.state.expiry.aad_access_exp) {
            let token =
                oauth::refresh_access_token(http, &tenant, self.aad_refresh.as_str(), SCOPE)
                    .await?;
            let claims = parse_claims(&token.id_token)?;
            self.aad_access = SecretString::new(token.access_token);
            self.aad_refresh = SecretString::new(token.refresh_token);
            self.aad_id = SecretString::new(token.id_token);
            self.state.expiry.aad_access_exp = claims.exp;
            changed = true;
        }

        if self.skype.is_empty() || is_expired(self.state.expiry.skype_exp) {
            let authz = skype::exchange_skype_token(http, self.aad_access.as_str()).await?;
            self.skype = SecretString::new(authz.tokens.skype_token);
            self.state.expiry.skype_exp = chrono::Utc::now().timestamp() + authz.tokens.expires_in;
            self.state.region_gtms = value_to_string_map(authz.region_gtms);
            changed = true;
        }

        if changed {
            self.persist()?;
        }
        Ok(())
    }

    pub fn persist(&self) -> Result<(), AuthError> {
        let paths = AppPaths::resolve()?;
        let store = store::default_store(&paths);
        store.set("aad_access", &self.aad_access)?;
        store.set("aad_refresh", &self.aad_refresh)?;
        store.set("aad_id", &self.aad_id)?;
        store.set("skype", &self.skype)?;
        self.state
            .save(&paths.state)
            .map_err(|error| AuthError::Config(error.to_string()))?;
        Ok(())
    }

    pub fn logout() -> Result<(), AuthError> {
        let paths = AppPaths::resolve()?;
        let store = store::default_store(&paths);
        for key in ["aad_access", "aad_refresh", "aad_id", "skype"] {
            let _ = store.delete(key);
        }
        if paths.state.exists() {
            std::fs::remove_file(paths.state)?;
        }
        Ok(())
    }
}

fn value_to_string_map(value: Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|text| (key.clone(), text.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}
