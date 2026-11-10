//! Shared entrypoint for the `speakflow` CLI and stdio MCP server.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::services::{http_api_service, meeting_service, search_service};

pub fn run_cli(args: Vec<String>) -> i32 {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("Failed to initialize CLI async runtime: {err}");
            return 1;
        }
    };

    rt.block_on(async move {
        let cmd = args.first().map(String::as_str).unwrap_or("--help");
        match cmd {
            "-h" | "--help" | "help" => {
                print_help();
                0
            }
            "status" => cmd_status(&args[1..]).await,
            "search" => cmd_search(&args[1..]).await,
            "list" => cmd_list(&args[1..]).await,
            "show" => cmd_show(&args[1..]).await,
            "meetings" => cmd_meetings(&args[1..]).await,
            "control" => cmd_control(&args[1..]).await,
            "stream" => cmd_stream().await,
            "mcp" => cmd_mcp().await,
            other => {
                eprintln!("Unknown speakflow subcommand: {other}\n");
                print_help();
                2
            }
        }
    })
}

fn print_help() {
    println!(
        "speakflow - SpeakFlow CLI, transcript search, live control, and MCP server\n\n\
         USAGE:\n  \
           speakflow status [--json]\n  \
           speakflow search <query> [--limit N] [--kind all|dictation|meeting] [--json]\n  \
           speakflow list [--limit N] [--kind all|dictation|meeting] [--json]\n  \
           speakflow show <id> [--json]\n  \
           speakflow meetings [--json]\n  \
           speakflow control <start_dictation|stop_dictation|toggle_dictation|start_meeting|stop_meeting|paste_last|open_typed_text> [--title TITLE] [--app APP]\n  \
           speakflow stream\n  \
           speakflow mcp"
    );
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    for i in 0..args.len() {
        if args[i] == name {
            return args.get(i + 1).cloned();
        }
        if let Some(rest) = args[i].strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
    }
    None
}

async fn api_get(path: &str) -> Result<Value, String> {
    let info = http_api_service::read_discovery().ok_or_else(|| {
        "SpeakFlow desktop app is not running (no local_api.json found)".to_string()
    })?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}{}", info.base_url, path);
    let resp = client
        .get(&url)
        .bearer_auth(&info.auth_token)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to local SpeakFlow API: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Local API returned HTTP {}", resp.status()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

async fn api_post(path: &str, body: Value) -> Result<Value, String> {
    let info = http_api_service::read_discovery().ok_or_else(|| {
        "SpeakFlow desktop app is not running (no local_api.json found)".to_string()
    })?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}{}", info.base_url, path);
    let resp = client
        .post(&url)
        .bearer_auth(&info.auth_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to local SpeakFlow API: {e}"))?;
    let status = resp.status();
    let payload: Value = resp
        .json()
        .await
        .unwrap_or_else(|_| json!({ "error": format!("HTTP {status}") }));
    if !status.is_success() {
        return Err(payload["error"]
            .as_str()
            .unwrap_or("Local API error")
            .to_string());
    }
    Ok(payload)
}

async fn cmd_status(args: &[String]) -> i32 {
    let as_json = has_flag(args, "--json");
    match api_get("/status").await {
        Ok(val) => {
            if as_json {
                println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
            } else {
                println!(
                    "SpeakFlow running: engine={}, ready={}, meetingRecording={}",
                    val["engine"].as_str().unwrap_or("unknown"),
                    val["funasrReady"].as_bool().unwrap_or(false),
                    val["meeting"]["recording"].as_bool().unwrap_or(false)
                );
            }
            0
        }
        Err(err) => {
            if as_json {
                println!("{}", json!({ "running": false, "error": err }));
            } else {
                println!("SpeakFlow daemon offline ({err})");
            }
            1
        }
    }
}

async fn cmd_search(args: &[String]) -> i32 {
    let as_json = has_flag(args, "--json");
    let limit = flag_value(args, "--limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(20);
    let kind = flag_value(args, "--kind").unwrap_or_else(|| "all".to_string());

    let mut terms = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--limit" || arg == "--kind" {
            skip_next = true;
            continue;
        }
        if arg == "--json" || arg.starts_with("--limit=") || arg.starts_with("--kind=") {
            continue;
        }
        terms.push(arg.as_str());
    }
    let query = terms.join(" ");
    match search_service::search_all(&query, &kind, limit).await {
        Ok(results) => {
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&results).unwrap_or_default()
                );
            } else if results.is_empty() {
                println!("No matching transcripts found.");
            } else {
                for r in results {
                    println!("[{}] {} ({:.2}) - {}", r.id, r.title, r.score, r.snippet);
                }
            }
            0
        }
        Err(err) => {
            eprintln!("Search error: {err}");
            1
        }
    }
}

async fn cmd_list(args: &[String]) -> i32 {
    let as_json = has_flag(args, "--json");
    let limit = flag_value(args, "--limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(25);
    let kind = flag_value(args, "--kind").unwrap_or_else(|| "all".to_string());
    match search_service::search_all("", &kind, limit).await {
        Ok(results) => {
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&results).unwrap_or_default()
                );
            } else {
                for r in results {
                    println!("[{}] {} - {}", r.id, r.title, r.snippet);
                }
            }
            0
        }
        Err(err) => {
            eprintln!("List error: {err}");
            1
        }
    }
}

async fn cmd_show(args: &[String]) -> i32 {
    let as_json = has_flag(args, "--json");
    let Some(id) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("Usage: speakflow show <id> [--json]");
        return 2;
    };
    if let Some(meeting_id) = id.strip_prefix("meeting:") {
        if let Some(m) = meeting_service::get_meeting_by_id(meeting_id) {
            if as_json {
                println!("{}", serde_json::to_string_pretty(&m).unwrap_or_default());
            } else {
                println!("{}", meeting_service::render_meeting_markdown(&m));
            }
            return 0;
        }
    }
    let clean = id.strip_prefix("dictation:").unwrap_or(id);
    if let Ok(num) = clean.parse::<i64>() {
        if let Ok(Some(stored)) = crate::services::history_service::get(num).await {
            if as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&stored.record).unwrap_or_default()
                );
            } else {
                println!("{}", stored.record.text);
            }
            return 0;
        }
    } else if let Some(m) = meeting_service::get_meeting_by_id(id) {
        if as_json {
            println!("{}", serde_json::to_string_pretty(&m).unwrap_or_default());
        } else {
            println!("{}", meeting_service::render_meeting_markdown(&m));
        }
        return 0;
    }
    eprintln!("Transcript '{id}' not found");
    1
}

async fn cmd_meetings(args: &[String]) -> i32 {
    let as_json = has_flag(args, "--json");
    let meetings = meeting_service::load_meetings();
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&meetings).unwrap_or_default()
        );
    } else if meetings.is_empty() {
        println!("No recorded meetings found.");
    } else {
        for m in meetings {
            println!(
                "[{}] {} ({:.0}s, {} speakers, {} words)",
                m.id, m.title, m.duration_sec, m.speaker_count, m.word_count
            );
        }
    }
    0
}

async fn cmd_control(args: &[String]) -> i32 {
    let Some(action) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("Usage: speakflow control <action> [--title TITLE] [--app APP]");
        return 2;
    };
    let body = json!({
        "action": action,
        "title": flag_value(args, "--title"),
        "appName": flag_value(args, "--app"),
    });
    match api_post("/control", body).await {
        Ok(val) => {
            println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
            0
        }
        Err(err) => {
            eprintln!("Control command failed: {err}");
            1
        }
    }
}

async fn cmd_stream() -> i32 {
    let Some(info) = http_api_service::read_discovery() else {
        eprintln!("SpeakFlow desktop app is not running (no local_api.json found)");
        return 1;
    };
    let client = match reqwest::Client::builder().build() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("HTTP client error: {err}");
            return 1;
        }
    };
    let url = format!("{}/streams", info.base_url);
    let mut resp = match client.get(&url).bearer_auth(&info.auth_token).send().await {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Failed to connect to {url}: {err}");
            return 1;
        }
    };
    while let Ok(Some(chunk)) = resp.chunk().await {
        print!("{}", String::from_utf8_lossy(&chunk));
        let _ = std::io::stdout().flush();
    }
    0
}

async fn cmd_mcp() -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(raw) = line else {
            break;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(err) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {err}") }
                });
                let _ = writeln!(stdout, "{err_resp}");
                let _ = stdout.flush();
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req["method"].as_str().unwrap_or("");
        let params = &req["params"];

        // Notifications without an id do not produce responses in JSON-RPC 2.0.
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "speakflow",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => Ok(json!({ "tools": mcp_tool_definitions() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let arguments = &params["arguments"];
                handle_mcp_tool_call(name, arguments).await
            }
            "ping" => Ok(json!({})),
            _ => Err((-32601, format!("Method not found: {method}"))),
        };

        if let Some(req_id) = id {
            let envelope = match result {
                Ok(res) => json!({ "jsonrpc": "2.0", "id": req_id, "result": res }),
                Err((code, msg)) => {
                    json!({ "jsonrpc": "2.0", "id": req_id, "error": { "code": code, "message": msg } })
                }
            };
            let _ = writeln!(stdout, "{envelope}");
            let _ = stdout.flush();
        }
    }
    0
}

fn mcp_tool_definitions() -> Value {
    json!([
        {
            "name": "search_transcripts",
            "description": "Search SpeakFlow dictation history and meeting transcripts using stemmed + fuzzy BM25 ranking.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search keywords or phrase" },
                    "kind": { "type": "string", "enum": ["all", "dictation", "meeting"], "default": "all" },
                    "limit": { "type": "integer", "default": 10 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "list_recent_transcripts",
            "description": "List recent dictations and meetings chronologically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["all", "dictation", "meeting"], "default": "all" },
                    "limit": { "type": "integer", "default": 20 }
                }
            }
        },
        {
            "name": "get_meeting",
            "description": "Read a recorded meeting transcript and AI notes by meeting ID.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        },
        {
            "name": "get_status",
            "description": "Check whether the SpeakFlow desktop daemon is running and inspect active ASR/meeting state.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "control_speakflow",
            "description": "Control the live SpeakFlow desktop app (start/stop/toggle dictation, start/stop meeting, paste last).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["start_dictation", "stop_dictation", "toggle_dictation", "start_meeting", "stop_meeting", "paste_last"]
                    },
                    "title": { "type": "string" },
                    "appName": { "type": "string" }
                },
                "required": ["action"]
            }
        }
    ])
}

async fn handle_mcp_tool_call(name: &str, args: &Value) -> Result<Value, (i32, String)> {
    match name {
        "search_transcripts" => {
            let query = args["query"].as_str().unwrap_or("");
            let kind = args["kind"].as_str().unwrap_or("all");
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            let hits = search_service::search_all(query, kind, limit)
                .await
                .map_err(|e| (-32000, e))?;
            Ok(mcp_text_result(
                &serde_json::to_string_pretty(&hits).unwrap_or_default(),
            ))
        }
        "list_recent_transcripts" => {
            let kind = args["kind"].as_str().unwrap_or("all");
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;
            let hits = search_service::search_all("", kind, limit)
                .await
                .map_err(|e| (-32000, e))?;
            Ok(mcp_text_result(
                &serde_json::to_string_pretty(&hits).unwrap_or_default(),
            ))
        }
        "get_meeting" => {
            let id = args["id"].as_str().unwrap_or("");
            match meeting_service::get_meeting_by_id(id) {
                Some(m) => Ok(mcp_text_result(&meeting_service::render_meeting_markdown(
                    &m,
                ))),
                None => Err((-32004, format!("Meeting '{id}' not found"))),
            }
        }
        "get_status" => match api_get("/status").await {
            Ok(v) => Ok(mcp_text_result(
                &serde_json::to_string_pretty(&v).unwrap_or_default(),
            )),
            Err(err) => Ok(mcp_text_result(
                &json!({ "running": false, "error": err }).to_string(),
            )),
        },
        "control_speakflow" => {
            let action = args["action"].as_str().unwrap_or("");
            let body = json!({
                "action": action,
                "title": args.get("title"),
                "appName": args.get("appName"),
            });
            let res = api_post("/control", body).await.map_err(|e| (-32000, e))?;
            Ok(mcp_text_result(
                &serde_json::to_string_pretty(&res).unwrap_or_default(),
            ))
        }
        other => Err((-32602, format!("Unknown MCP tool: {other}"))),
    }
}

fn mcp_text_result(text: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_entry_skips_the_program_name_before_dispatch() {
        // Regression: dispatching on argv[0] made every subcommand report
        // "Unknown speakflow subcommand: <exe path>".
        let entry =
            |args: &[&str]| crate::cli_entry(args.iter().map(|arg| (*arg).to_string()).collect());
        assert_eq!(entry(&["speakflow", "--help"]), 0);
        assert_eq!(entry(&["speakflow", "bogus"]), 2);
    }

    #[test]
    fn mcp_tool_schema_contains_all_expected_tools() {
        let tools = mcp_tool_definitions();
        let arr = tools.as_array().unwrap();
        let names: Vec<&str> = arr.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"search_transcripts"));
        assert!(names.contains(&"list_recent_transcripts"));
        assert!(names.contains(&"get_meeting"));
        assert!(names.contains(&"get_status"));
        assert!(names.contains(&"control_speakflow"));
    }
}
