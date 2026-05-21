use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;
use crate::util::chat_cache;
use crate::util::json::print_pretty;
use serde_json::json;

pub async fn run(force: bool, tenant: &str, json_output: bool) -> Result<(), CliError> {
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    if !force {
        let paths = AppPaths::resolve()?;
        if paths.state.exists() {
            match Session::load(&http).await {
                Ok(session) => {
                    if json_output {
                        print_pretty(&json!({
                            "ok": true,
                            "logged_in": true,
                            "already_logged_in": true,
                            "identity": {
                                "display_name": session.state.identity.display_name,
                                "upn": session.state.identity.upn,
                                "object_id": session.state.identity.user_oid,
                                "tenant_id": session.state.identity.tenant_id
                            }
                        }))?;
                    } else {
                        println!(
                            "Already logged in as {}",
                            session.state.identity.display_name
                        );
                    }
                    return Ok(());
                }
                Err(error) => tracing::debug!("cached session is not usable: {error}"),
            }
        }
    }

    let session = Session::login_interactive(&http, tenant).await?;
    let paths = AppPaths::resolve()?;
    let cache_cleared = chat_cache::clear(&paths)?;
    if json_output {
        print_pretty(&json!({
            "ok": true,
            "logged_in": true,
            "already_logged_in": false,
            "cache_cleared": cache_cleared,
            "identity": {
                "display_name": session.state.identity.display_name,
                "upn": session.state.identity.upn,
                "object_id": session.state.identity.user_oid,
                "tenant_id": session.state.identity.tenant_id
            }
        }))?;
    } else {
        println!(
            "Logged in as {} <{}> (tenant {})",
            session.state.identity.display_name,
            session.state.identity.upn,
            session.state.identity.tenant_id
        );
    }
    Ok(())
}
