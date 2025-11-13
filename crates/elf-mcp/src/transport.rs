use anyhow::{anyhow, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};

use crate::tools::ToolRegistry;

/// Transport implementation for MCP requests.
pub fn run(registry: &ToolRegistry, transport: &str) -> Result<()> {
    match transport {
        "stdio" => run_stdio(registry),
        other => Err(anyhow!("unsupported transport '{}'", other)),
    }
}

fn run_stdio(registry: &ToolRegistry) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    info!("Listening for MCP requests on stdin/stdout");
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            info!("EOF reached, stopping MCP transport");
            break;
        }

        if line.trim().is_empty() {
            continue;
        }

        let request: ToolRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(err) => {
                error!("failed to parse request: {}", err);
                let response = ToolResponse::error(None, err.to_string());
                write_response(&mut writer, &response)?;
                continue;
            }
        };

        let response = match registry.execute(&request.method, request.params) {
            Ok(result) => ToolResponse::ok(request.id, result),
            Err(err) => {
                error!("tool '{}' failed: {}", request.method, err);
                ToolResponse::error(request.id, err.to_string())
            }
        };

        write_response(&mut writer, &response)?;
    }

    Ok(())
}

fn write_response(writer: &mut dyn Write, response: &ToolResponse) -> Result<()> {
    let serialized = serde_json::to_string(response)?;
    writer.write_all(serialized.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ToolRequest {
    #[serde(rename = "id")]
    pub id: Option<String>,
    #[serde(rename = "method")]
    pub method: String,
    #[serde(rename = "params")]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct ToolResponse {
    pub id: Option<String>,
    pub result: Option<Value>,
    pub error: Option<String>,
}

impl ToolResponse {
    fn ok(id: Option<String>, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<String>, err: String) -> Self {
        Self {
            id,
            result: None,
            error: Some(err),
        }
    }
}
