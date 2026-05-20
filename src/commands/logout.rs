use crate::auth::Session;
use crate::error::CliError;

pub async fn run() -> Result<(), CliError> {
    Session::logout()?;
    println!("Logged out. (No server-side revocation; tokens expire naturally.)");
    Ok(())
}
