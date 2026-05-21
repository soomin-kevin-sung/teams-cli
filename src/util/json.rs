use crate::error::CliError;
use serde::Serialize;

pub fn print_pretty<T: Serialize>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
