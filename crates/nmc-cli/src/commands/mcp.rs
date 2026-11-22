//! `nmc mcp`.

use std::process::ExitCode;

use nmc_app::AppContext;
use nmc_mcp::{Grants, Server};

use super::CmdResult;
use crate::cli::McpArgs;
use crate::location;

/// Grants from the command line and the grants file.
fn grants(args: &McpArgs) -> Result<Grants, Box<dyn std::error::Error>> {
    let mut read = args.read.clone();
    let mut write = args.write.clone();
    if let Some(file) = &args.grants {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let list = |key: &str| -> Vec<String> {
            json.get(key)
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
                .unwrap_or_default()
        };
        read.extend(list("read"));
        write.extend(list("write"));
    }
    let mut grants = Grants::none();
    for text in &read {
        grants.allow_read(location::parse(text)?);
    }
    for text in &write {
        grants.allow_write(location::parse(text)?);
    }
    Ok(grants)
}

pub async fn mcp(context: &AppContext, args: McpArgs) -> CmdResult {
    let grants = grants(&args)?;
    let mut server = Server::new(context.registry.clone(), grants);
    if let Some(file) = &args.log {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .map_err(|e| format!("{}: {e}", file.display()))?;
        server = server.with_log(Box::new(log));
    }
    // Standard output carries the protocol: nothing else may be printed.
    let input = tokio::io::BufReader::new(tokio::io::stdin());
    server.serve(input, tokio::io::stdout()).await?;
    Ok(ExitCode::SUCCESS)
}
