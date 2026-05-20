use crate::auth::{Session, USER_AGENT};
use crate::config::AppPaths;
use crate::error::CliError;

pub async fn run(force: bool, tenant: &str) -> Result<(), CliError> {
    let http = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    if !force {
        let paths = AppPaths::resolve()?;
        if paths.state.exists() {
            match Session::load(&http).await {
                Ok(session) => {
                    println!(
                        "Already logged in as {}",
                        session.state.identity.display_name
                    );
                    return Ok(());
                }
                Err(error) => tracing::debug!("cached session is not usable: {error}"),
            }
        }
    }

    let session = Session::login_interactive(&http, tenant).await?;
    println!(
        "Logged in as {} <{}> (tenant {})",
        session.state.identity.display_name,
        session.state.identity.upn,
        session.state.identity.tenant_id
    );
    Ok(())
}
