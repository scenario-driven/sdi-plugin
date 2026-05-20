//! Output formatting helpers. Every entity-emitting subcommand prints either a
//! JSON object (default) or, with `--quiet`, just the entity id.

use anyhow::Result;
use serde_json::Value;

pub fn emit(value: &Value, quiet: bool) -> Result<()> {
    if quiet {
        if let Some(id) = value.get("id").and_then(|v| v.as_str()) {
            println!("{}", id);
            return Ok(());
        }
    }
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
