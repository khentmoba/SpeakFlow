//! Local loopback HTTP/1.1 + Server-Sent Events (SSE) server for SpeakFlow.
//!
//! Exposes:
//! - `GET /health`
//! - `GET /status`
//! - `GET /streams` (SSE live dictation & meeting stream)
//! - `GET /transcriptions?q=...&limit=...&kind=all|dictation|meeting`
//! - `GET /transcriptions/:id`
//! - `GET /meetings`
//! - `GET /meetings/:id`
//! - `POST /search`
//! - `POST /control`
//! - `POST /meetings/events`
//!
//! Binds strictly to `127.0.0.1` and writes a discovery descriptor at
//! `<data_dir>/local_api.json` with `port`, `pid`, and `authToken`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

use crate::commands;
use crate::services::{history_service, meeting_service, search_service};
use crate::state::{AppState, RecordingTrigger};
use crate::utils::paths;

pub const DEFAULT_API_PORT: u16 = 3847;
const LOCAL_API_FILE: &str = "local_api.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiInfo {
    pub host: String,
    pub port: u16,
    pub pid: u32,
    pub started_at: u64,
    pub auth_token: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamEventEnvelope {
    pub event: String,
    pub timestamp_ms: u64,
    pub data: serde_json::Value,
}

static STREAM_BUS: OnceLock<broadcast::Sender<StreamEventEnvelope>> = OnceLock::new();
static ACTIVE_INFO: Mutex<Option<LocalApiInfo>> = Mutex::new(None);

/// The loopback listener that currently owns the port. Sending on `shutdown`
/// ends its accept loop; `id` lets a listener that lost a stop race tell that
/// it no longer owns the slot.
struct ActiveListener {
    id: u64,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

static LISTENER: Mutex<Option<ActiveListener>> = Mutex::new(None);
static NEXT_LISTENER_ID: AtomicU64 = AtomicU64::new(1);

fn stream_sender() -> &'static broadcast::Sender<StreamEventEnvelope> {
    STREAM_BUS.get_or_init(|| {
        let (tx, _) = broadcast::channel(128);
        tx
    })
}

pub fn publish_stream_event(event: &str, data: serde_json::Value) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let _ = stream_sender().send(StreamEventEnvelope {
        event: event.to_string(),
        timestamp_ms: ts,
        data,
    });
}

pub fn discovery_path() -> PathBuf {
    paths::get_data_dir().join(LOCAL_API_FILE)
}

pub fn read_discovery() -> Option<LocalApiInfo> {
    if let Some(info) = ACTIVE_INFO.lock().clone() {
        return Some(info);
    }
    let raw = std::fs::read_to_string(discovery_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn generate_auth_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Starts the loopback API unless it is already running. Called at startup and
/// when Labs > Local API is switched on.
pub fn start_server(app_handle: tauri::AppHandle) {
    let id = NEXT_LISTENER_ID.fetch_add(1, Ordering::Relaxed);
    let Some(shutdown_rx) = install_listener(&mut LISTENER.lock(), id) else {
        return;
    };
    tauri::async_runtime::spawn(run_listener(app_handle, id, shutdown_rx));
}

/// Stops the loopback API now. The auth token and discovery file go with it, so
/// a disabled API cannot keep answering until the next restart.
pub fn stop_server() {
    let Some(active) = LISTENER.lock().take() else {
        return;
    };
    let _ = active.shutdown.send(());
    *ACTIVE_INFO.lock() = None;
    let _ = std::fs::remove_file(discovery_path());
    log::info!("SpeakFlow local HTTP API stopped");
}

fn install_listener(
    slot: &mut Option<ActiveListener>,
    id: u64,
) -> Option<tokio::sync::oneshot::Receiver<()>> {
    if slot.is_some() {
        return None;
    }
    let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
    *slot = Some(ActiveListener { id, shutdown });
    Some(shutdown_rx)
}

fn release_listener(id: u64) {
    let mut slot = LISTENER.lock();
    if slot.as_ref().is_some_and(|active| active.id == id) {
        *slot = None;
    }
}

async fn bind_listener(configured_port: u16) -> Option<TcpListener> {
    match TcpListener::bind(("127.0.0.1", configured_port)).await {
        Ok(listener) => Some(listener),
        Err(first_err) => {
            log::warn!(
                "Local API port {} unavailable ({first_err}), binding ephemeral loopback port",
                configured_port
            );
            match TcpListener::bind(("127.0.0.1", 0)).await {
                Ok(listener) => Some(listener),
                Err(err) => {
                    log::error!("Failed to bind local HTTP API listener: {err}");
                    None
                }
            }
        }
    }
}

async fn run_listener(
    app_handle: tauri::AppHandle,
    id: u64,
    mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let configured_port = std::env::var("SPEAK_FLOW_API_PORT")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_API_PORT);

    let bound = tokio::select! {
        biased;
        _ = &mut shutdown_rx => return,
        bound = bind_listener(configured_port) => bound,
    };
    let Some(listener) = bound else {
        release_listener(id);
        return;
    };
    let Ok(addr) = listener.local_addr() else {
        release_listener(id);
        return;
    };
    let port = addr.port();
    let info = LocalApiInfo {
        host: "127.0.0.1".to_string(),
        port,
        pid: std::process::id(),
        started_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        auth_token: generate_auth_token(),
        base_url: format!("http://127.0.0.1:{port}"),
    };
    {
        // A stop that raced the bind has already cleared the slot. Publishing
        // now would hand out a token for a listener that is about to close.
        let slot = LISTENER.lock();
        if slot.as_ref().is_none_or(|active| active.id != id) {
            return;
        }
        *ACTIVE_INFO.lock() = Some(info.clone());
        if let Ok(json) = serde_json::to_string_pretty(&info) {
            let _ = paths::atomic_write(&discovery_path(), json.as_bytes());
        }
    }
    log::info!("SpeakFlow local HTTP API listening on http://127.0.0.1:{port}");

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown_rx => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _peer)) => {
                    let app = app_handle.clone();
                    tokio::spawn(async move {
                        if let Err(err) = handle_connection(stream, app).await {
                            log::debug!("Local HTTP connection closed: {err}");
                        }
                    });
                }
                Err(err) => {
                    log::warn!("Local HTTP accept error: {err}");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            },
        }
    }
    log::info!("SpeakFlow local HTTP API closed on port {port}");
}

fn is_allowed_origin(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let lower = origin.trim().to_ascii_lowercase();
    lower.starts_with("http://localhost")
        || lower.starts_with("http://127.0.0.1")
        || lower.starts_with("https://tauri.localhost")
        || lower.starts_with("http://tauri.localhost")
        || lower.starts_with("tauri://localhost")
}

async fn handle_connection(stream: TcpStream, app_handle: tauri::AppHandle) -> Result<(), String> {
    let (reader_half, mut writer_half) = stream.into_split();
    let mut reader = BufReader::new(reader_half);

    let mut request_line = String::new();
    let n = reader
        .read_line(&mut request_line)
        .await
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        write_json_response(
            &mut writer_half,
            400,
            serde_json::json!({ "error": "Bad request" }),
        )
        .await?;
        return Ok(());
    }
    let method = parts[0].to_ascii_uppercase();
    let target = parts[1];

    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    if !is_allowed_origin(headers.get("origin").map(String::as_str)) {
        write_json_response(
            &mut writer_half,
            403,
            serde_json::json!({ "error": "Cross-origin browser requests are blocked" }),
        )
        .await?;
        return Ok(());
    }

    let content_len = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(2 * 1024 * 1024);
    let mut body = vec![0u8; content_len];
    if content_len > 0 {
        reader
            .read_exact(&mut body)
            .await
            .map_err(|e| e.to_string())?;
    }

    let (path, query_str) = target.split_once('?').unwrap_or((target, ""));
    let query = parse_query_params(query_str);

    // Require valid bearer auth on all routes except unauthenticated `GET /health`.
    // Local CLI/tools read `local_api.json` automatically and supply `Authorization: Bearer <token>`.
    let requires_auth = !(method == "GET" && path == "/health");
    if requires_auth && !is_authorized_bearer(headers.get("authorization").map(String::as_str)) {
        write_json_response(
            &mut writer_half,
            401,
            serde_json::json!({ "error": "Unauthorized: valid bearer token required" }),
        )
        .await?;
        return Ok(());
    }

    if method == "GET" && path == "/streams" {
        return handle_sse_stream(writer_half, &app_handle).await;
    }

    match (method.as_str(), path) {
        ("GET", "/health") => {
            let state = app_handle.state::<AppState>();
            let info = read_discovery();
            write_json_response(
                &mut writer_half,
                200,
                serde_json::json!({
                    "ok": true,
                    "version": env!("CARGO_PKG_VERSION"),
                    "pid": std::process::id(),
                    "port": info.map(|i| i.port).unwrap_or(DEFAULT_API_PORT),
                    "engine": paths::read_engine_config(),
                    "ready": state.is_funasr_ready(),
                }),
            )
            .await?;
        }
        ("GET", "/status") => {
            let state = app_handle.state::<AppState>();
            let rec = state.recording.snapshot();
            let meeting = meeting_service::current_status();
            write_json_response(
                &mut writer_half,
                200,
                serde_json::json!({
                    "recording": rec,
                    "meeting": meeting,
                    "engine": paths::read_engine_config(),
                    "funasrReady": state.is_funasr_ready(),
                    "supportsNativeStreaming": state.engine.supports_native_streaming.load(std::sync::atomic::Ordering::Acquire),
                    "supportsDiarization": state.engine.supports_diarization.load(std::sync::atomic::Ordering::Acquire),
                    "localOnly": paths::read_local_only(),
                }),
            )
            .await?;
        }
        ("GET", "/transcriptions") => {
            let q = query.get("q").map(String::as_str).unwrap_or("");
            let kind = query.get("kind").map(String::as_str).unwrap_or("all");
            let limit = query
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(25)
                .clamp(1, 200);
            match search_service::search_all(q, kind, limit).await {
                Ok(items) => {
                    write_json_response(
                        &mut writer_half,
                        200,
                        serde_json::json!({
                            "items": items,
                            "count": items.len(),
                        }),
                    )
                    .await?;
                }
                Err(err) => {
                    write_json_response(&mut writer_half, 500, serde_json::json!({ "error": err }))
                        .await?;
                }
            }
        }
        ("POST", "/search") => {
            let req: SearchPostRequest = serde_json::from_slice(&body).unwrap_or_default();
            let limit = req.limit.unwrap_or(25).clamp(1, 200);
            let kind = req.kind.as_deref().unwrap_or("all");
            match search_service::search_all(&req.query, kind, limit).await {
                Ok(items) => {
                    write_json_response(
                        &mut writer_half,
                        200,
                        serde_json::json!({ "items": items, "count": items.len() }),
                    )
                    .await?;
                }
                Err(err) => {
                    write_json_response(&mut writer_half, 500, serde_json::json!({ "error": err }))
                        .await?;
                }
            }
        }
        ("GET", _) if path.starts_with("/transcriptions/") => {
            let raw_id = path.trim_start_matches("/transcriptions/");
            let clean_id = raw_id.strip_prefix("dictation:").unwrap_or(raw_id);
            if let Some(meeting_id) = raw_id.strip_prefix("meeting:") {
                match meeting_service::get_meeting_by_id(meeting_id) {
                    Some(m) => {
                        write_json_response(&mut writer_half, 200, serde_json::json!(m)).await?
                    }
                    None => {
                        write_json_response(
                            &mut writer_half,
                            404,
                            serde_json::json!({ "error": "Transcription not found" }),
                        )
                        .await?
                    }
                }
            } else if let Ok(num_id) = clean_id.parse::<i64>() {
                match history_service::get(num_id).await {
                    Ok(Some(stored)) => {
                        write_json_response(&mut writer_half, 200, serde_json::json!(stored.record))
                            .await?
                    }
                    Ok(None) => {
                        write_json_response(
                            &mut writer_half,
                            404,
                            serde_json::json!({ "error": "Transcription not found" }),
                        )
                        .await?
                    }
                    Err(err) => {
                        write_json_response(
                            &mut writer_half,
                            500,
                            serde_json::json!({ "error": err }),
                        )
                        .await?
                    }
                }
            } else {
                write_json_response(
                    &mut writer_half,
                    400,
                    serde_json::json!({ "error": "Invalid transcription id" }),
                )
                .await?;
            }
        }
        ("GET", "/meetings") => {
            let meetings = meeting_service::load_meetings();
            write_json_response(
                &mut writer_half,
                200,
                serde_json::json!({ "items": meetings, "count": meetings.len() }),
            )
            .await?;
        }
        ("GET", _) if path.starts_with("/meetings/") => {
            let id = path.trim_start_matches("/meetings/");
            match meeting_service::get_meeting_by_id(id) {
                Some(m) => {
                    write_json_response(&mut writer_half, 200, serde_json::json!(m)).await?;
                }
                None => {
                    write_json_response(
                        &mut writer_half,
                        404,
                        serde_json::json!({ "error": "Meeting not found" }),
                    )
                    .await?;
                }
            }
        }
        ("POST", "/control") => {
            let payload: ControlRequest = match serde_json::from_slice(&body) {
                Ok(req) => req,
                Err(err) => {
                    write_json_response(
                        &mut writer_half,
                        400,
                        serde_json::json!({ "error": format!("Invalid JSON body: {err}") }),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let result = execute_control_action(&app_handle, payload).await;
            match result {
                Ok(data) => write_json_response(&mut writer_half, 200, data).await?,
                Err(err) => {
                    write_json_response(&mut writer_half, 400, serde_json::json!({ "error": err }))
                        .await?
                }
            }
        }
        ("POST", "/meetings/events") => {
            let payload: MeetingEventRequest = match serde_json::from_slice(&body) {
                Ok(req) => req,
                Err(err) => {
                    write_json_response(
                        &mut writer_half,
                        400,
                        serde_json::json!({ "error": format!("Invalid JSON body: {err}") }),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let state = app_handle.state::<AppState>();
            let resp = match payload.event.trim().to_ascii_lowercase().as_str() {
                "start" | "call_started" => meeting_service::start_meeting(
                    &app_handle,
                    state.inner(),
                    payload.title,
                    payload.app_name,
                )
                .map(|status| serde_json::json!({ "ok": true, "status": status })),
                "stop" | "call_ended" => meeting_service::stop_meeting(&app_handle)
                    .await
                    .map(|record| serde_json::json!({ "ok": true, "meeting": record })),
                other => Err(format!("Unsupported meeting event: {other}")),
            };
            match resp {
                Ok(data) => write_json_response(&mut writer_half, 200, data).await?,
                Err(err) => {
                    write_json_response(&mut writer_half, 400, serde_json::json!({ "error": err }))
                        .await?
                }
            }
        }
        _ => {
            write_json_response(
                &mut writer_half,
                404,
                serde_json::json!({ "error": "Not found" }),
            )
            .await?;
        }
    }

    Ok(())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchPostRequest {
    #[serde(default)]
    query: String,
    kind: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControlRequest {
    action: String,
    title: Option<String>,
    app_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeetingEventRequest {
    event: String,
    title: Option<String>,
    app_name: Option<String>,
}

async fn execute_control_action(
    app_handle: &tauri::AppHandle,
    req: ControlRequest,
) -> Result<serde_json::Value, String> {
    let state = app_handle.state::<AppState>();
    match req.action.trim().to_ascii_lowercase().as_str() {
        "start_dictation" => {
            let sid = commands::audio::start_recording_inner(
                app_handle.clone(),
                state.inner(),
                RecordingTrigger::DictationOriginal,
                None,
            )
            .await
            .map_err(|e| e.to_string())?;
            publish_stream_event("dictation.started", serde_json::json!({ "sessionId": sid }));
            Ok(serde_json::json!({ "ok": true, "sessionId": sid }))
        }
        "stop_dictation" => {
            let sid =
                commands::audio::stop_recording_inner(app_handle.clone(), state.inner(), None)
                    .await
                    .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true, "sessionId": sid }))
        }
        "toggle_dictation" => {
            let is_active = state.recording.recording.lock().is_some();
            if is_active {
                let sid =
                    commands::audio::stop_recording_inner(app_handle.clone(), state.inner(), None)
                        .await
                        .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({ "ok": true, "state": "stopped", "sessionId": sid }))
            } else {
                let sid = commands::audio::start_recording_inner(
                    app_handle.clone(),
                    state.inner(),
                    RecordingTrigger::DictationOriginal,
                    None,
                )
                .await
                .map_err(|e| e.to_string())?;
                publish_stream_event("dictation.started", serde_json::json!({ "sessionId": sid }));
                Ok(serde_json::json!({ "ok": true, "state": "started", "sessionId": sid }))
            }
        }
        "start_meeting" => {
            let status =
                meeting_service::start_meeting(app_handle, state.inner(), req.title, req.app_name)?;
            Ok(serde_json::json!({ "ok": true, "meeting": status }))
        }
        "stop_meeting" => {
            let rec = meeting_service::stop_meeting(app_handle).await?;
            Ok(serde_json::json!({ "ok": true, "meeting": rec }))
        }
        "paste_last" => {
            let msg = commands::clipboard::paste_last_dictation(app_handle.clone())
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({ "ok": true, "message": msg }))
        }
        "open_typed_text" => {
            let _ = app_handle.emit("open-typed-text", ());
            Ok(serde_json::json!({ "ok": true }))
        }
        other => Err(format!("Unsupported control action: {other}")),
    }
}

async fn handle_sse_stream(
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    let state = app_handle.state::<AppState>();
    let snapshot = serde_json::json!({
        "recording": state.recording.snapshot(),
        "meeting": meeting_service::current_status(),
        "engine": paths::read_engine_config(),
    });
    let init_frame = format!(
        "event: connected\ndata: {}\n\n",
        serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())
    );
    writer
        .write_all(init_frame.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())?;

    let mut rx = stream_sender().subscribe();
    let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(15));
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(envelope) => {
                        let json = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
                        let frame = format!("event: {}\ndata: {}\n\n", envelope.event, json);
                        if writer.write_all(frame.as_bytes()).await.is_err() {
                            break;
                        }
                        let _ = writer.flush().await;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = keepalive.tick() => {
                if writer.write_all(b": keepalive\n\n").await.is_err() {
                    break;
                }
                let _ = writer.flush().await;
            }
        }
    }
    Ok(())
}

async fn write_json_response(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    status: u16,
    body: serde_json::Value,
) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let json = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
        json.len()
    );
    writer
        .write_all(resp.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}

fn parse_query_params(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(percent_decode(k), percent_decode(v));
    }
    map
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
            out.push(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_authorized_bearer(auth_header: Option<&str>) -> bool {
    let expected_token = ACTIVE_INFO
        .lock()
        .as_ref()
        .map(|i| i.auth_token.clone())
        .unwrap_or_default();
    if expected_token.is_empty() {
        return false;
    }
    let supplied = auth_header
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    !supplied.is_empty() && supplied == expected_token
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_handles_spaces_and_query_encoding() {
        let params = parse_query_params("q=hello+world%21&limit=10&kind=meeting");
        assert_eq!(params.get("q").map(String::as_str), Some("hello world!"));
        assert_eq!(params.get("limit").map(String::as_str), Some("10"));
        assert_eq!(params.get("kind").map(String::as_str), Some("meeting"));
    }

    #[test]
    fn origin_guard_blocks_external_web_origins_and_allows_local_clients() {
        assert!(is_allowed_origin(None));
        assert!(is_allowed_origin(Some("http://localhost:1420")));
        assert!(is_allowed_origin(Some("http://127.0.0.1:3847")));
        assert!(is_allowed_origin(Some("tauri://localhost")));
        assert!(!is_allowed_origin(Some("https://evil.example.com")));
    }

    #[test]
    fn bearer_auth_accepts_matching_token_and_rejects_missing_or_wrong() {
        let prev = ACTIVE_INFO.lock().clone();
        *ACTIVE_INFO.lock() = Some(LocalApiInfo {
            host: "127.0.0.1".into(),
            port: 3847,
            pid: 1,
            started_at: 0,
            auth_token: "secret-token-123".into(),
            base_url: "http://127.0.0.1:3847".into(),
        });
        assert!(is_authorized_bearer(Some("Bearer secret-token-123")));
        assert!(!is_authorized_bearer(None));
        assert!(!is_authorized_bearer(Some("secret-token-123")));
        assert!(!is_authorized_bearer(Some("Bearer wrong")));
        *ACTIVE_INFO.lock() = prev;
    }

    #[test]
    fn listener_slot_has_one_owner_until_it_is_stopped() {
        let mut slot = None;
        let mut first = install_listener(&mut slot, 1).expect("first listener claims the slot");
        assert!(install_listener(&mut slot, 2).is_none());

        slot.take()
            .expect("slot holds the first listener")
            .shutdown
            .send(())
            .expect("listener is still waiting");
        assert!(first.try_recv().is_ok());
        assert!(install_listener(&mut slot, 3).is_some());
    }
}
