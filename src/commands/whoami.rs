use crate::auth::AuthError;
use crate::config::{AppPaths, State};
use crate::error::CliError;
use crate::util::json::print_pretty;
use chrono::{TimeZone, Utc};
use serde_json::json;

pub async fn run(json_output: bool) -> Result<(), CliError> {
    let paths = AppPaths::resolve()?;
    let state = State::load(&paths.state)
        .map_err(|error| CliError::Other(error.to_string()))?
        .ok_or(AuthError::NotLoggedIn)?;

    if json_output {
        print_pretty(&json!({
            "ok": true,
            "identity": {
                "display_name": state.identity.display_name,
                "upn": state.identity.upn,
                "object_id": state.identity.user_oid,
                "tenant_id": state.identity.tenant_id
            },
            "expiry": {
                "aad_access_exp": state.expiry.aad_access_exp,
                "skype_exp": state.expiry.skype_exp
            },
            "region_gtms": state.region_gtms
        }))?;
        return Ok(());
    }

    println!("Display name : {}", state.identity.display_name);
    println!("UPN          : {}", state.identity.upn);
    println!("Object id    : {}", state.identity.user_oid);
    println!("Tenant       : {}", state.identity.tenant_id);
    println!(
        "AAD token    : {} ({})",
        expires_in(state.expiry.aad_access_exp),
        format_epoch(state.expiry.aad_access_exp)
    );
    println!("Skype token  : {}", expires_in(state.expiry.skype_exp));
    if let Some(chat_service) = state.region_gtms.get("chatService") {
        println!("Region       : chatService={chat_service}");
    }
    Ok(())
}

fn expires_in(exp: i64) -> String {
    let seconds = exp - Utc::now().timestamp();
    if seconds <= 0 {
        return "expired".to_string();
    }
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("expires in {hours}h {minutes}m")
    } else {
        format!("expires in {minutes}m")
    }
}

fn format_epoch(exp: i64) -> String {
    Utc.timestamp_opt(exp, 0)
        .single()
        .map(|time| time.to_rfc3339())
        .unwrap_or_else(|| "invalid timestamp".to_string())
}
