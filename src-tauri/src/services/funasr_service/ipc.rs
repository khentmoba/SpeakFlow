use super::*;

/// Send a command to the Python server and read the response
///
/// This is the core function for talking to the Python process.
///
/// # Flow
/// 1. Hold the process lock and take the child process out of global state
/// 2. Serialize the command to JSON
/// 3. Write the JSON + newline to stdin
/// 4. Read one line of JSON response from stdout
/// 5. Put the child process back into global state
/// 6. Deserialize the response and return it
///
/// # Notes
/// - Every message must end with a newline
/// - Commands and responses are both single-line JSON
/// - To guarantee only one command talks to the child process at a time,
///   the lock is held until I/O completes, which prevents concurrent reads/writes from corrupting the protocol.
pub(super) async fn send_command_to_server<R: tauri::Runtime>(
    state: &AppState,
    command: &ServerCommand,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> Result<ServerResponse, AppError> {
    send_command_to_server_with_timeout(
        state,
        command,
        app_handle,
        Duration::from_secs(SERVER_RESPONSE_TIMEOUT_SECS),
    )
    .await
}

// The process stays outside the shared slot until its complete reply arrives.
// Cancellation can leave a partial JSON frame, so only success restores it.
pub(super) struct CommandProcessGuard<'a, R: tauri::Runtime> {
    state: &'a AppState,
    app_handle: Option<&'a tauri::AppHandle<R>>,
    process: Option<FunasrProcess>,
}

impl<R: tauri::Runtime> Drop for CommandProcessGuard<'_, R> {
    fn drop(&mut self) {
        let Some(process) = self.process.take() else {
            return;
        };
        // The process can outlive a timeout or broken pipe (a stalled decode, for example).
        // Its reply can no longer be matched to a session, and a leftover copy would compete
        // with the replacement for GPU memory, so the recovery task ends it first.
        log::warn!("FunASR communication interrupted; releasing the process for restart");
        self.state.set_funasr_ready(false);
        self.state.set_inline_audio_transport(None);
        if let Some(handle) = self.app_handle {
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "crashed",
                    "message": "Speech recognition engine disconnected; preparing to restart..."
                }),
            );
        }
        // Recovery reads its own app handle, so it must run even for commands sent
        // without one; otherwise those failures leave the engine down for good.
        request_engine_recovery(Some(process));
    }
}

pub(super) async fn send_command_to_server_with_timeout<R: tauri::Runtime>(
    state: &AppState,
    command: &ServerCommand,
    app_handle: Option<&tauri::AppHandle<R>>,
    timeout: Duration,
) -> Result<ServerResponse, AppError> {
    // One budget covers waiting for the lock, write, flush, and the reply.
    tokio::time::timeout(timeout, async {
        let mut guard = state.engine.funasr_process.lock().await;
        let process = guard
            .take()
            .ok_or_else(|| AppError::Asr("FunASR process is not running".to_string()))?;
        let mut owned = CommandProcessGuard {
            state,
            app_handle,
            process: Some(process),
        };
        let result = match owned.process.as_mut() {
            Some(process) => send_command_impl(process, command).await,
            None => Err(AppError::Asr("FunASR process is not running".to_string())),
        };
        if result.is_ok() {
            // A valid application error is still a healthy transport reply.
            *guard = owned.process.take();
        }
        result
    })
    .await
    .map_err(|_| AppError::Asr("Timed out waiting for the FunASR command".to_string()))?
}

#[derive(serde::Serialize)]
struct CommandEnvelope<'a> {
    request_id: u64,
    #[serde(flatten)]
    command: &'a ServerCommand,
}

/// Internal implementation of sending a command to the child process and reading the response
///
/// The caller holds the process lock and the cancel guard for the whole I/O.
pub(super) async fn send_command_impl(
    process: &mut FunasrProcess,
    command: &ServerCommand,
) -> Result<ServerResponse, AppError> {
    let request_id = NEXT_SERVER_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    // Serialize straight to a byte stream once, avoiding to_value (a deep copy of the 512KB base64 payload) then to_string then format
    let mut command_bytes = serde_json::to_vec(&CommandEnvelope {
        request_id,
        command,
    })
    .map_err(|e| AppError::Asr(format!("Failed to serialize command: {}", e)))?;
    command_bytes.push(b'\n');

    // Write the command to stdin
    // `write_all` ensures all bytes are written
    process
        .stdin
        .write_all(&command_bytes)
        .await
        .map_err(|e| AppError::Asr(format!("Failed to write command to FunASR: {}", e)))?;

    // `flush` ensures buffered data is sent immediately
    process
        .stdin
        .flush()
        .await
        .map_err(|e| AppError::Asr(format!("Failed to flush stdin buffer: {}", e)))?;

    // Read the response from stdout (non-JSON lines may be skipped)
    read_json_response_matching(
        &mut process.stdout,
        Duration::from_secs(SERVER_RESPONSE_TIMEOUT_SECS),
        "Waiting for FunASR response",
        |response: &ServerResponse| {
            server_response_matches_request(response.request_id, request_id)
        },
    )
    .await
}

pub(super) fn server_response_matches_request(actual: Option<u64>, expected: u64) -> bool {
    // Every bundled server echoes request_id. An untagged response cannot be
    // distinguished from a late reply after cancellation or timeout.
    actual == Some(expected)
}
