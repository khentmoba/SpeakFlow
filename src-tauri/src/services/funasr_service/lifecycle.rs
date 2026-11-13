use super::*;

/// Lets the IPC layer restart the engine without a command-scoped app handle.
static ENGINE_RECOVERY_APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

pub fn register_engine_recovery_app(app_handle: tauri::AppHandle) {
    let _ = ENGINE_RECOVERY_APP.set(app_handle);
}

/// How long recovery waits for a lost process to exit before starting a replacement.
const LOST_PROCESS_EXIT_BUDGET: Duration = Duration::from_secs(5);

/// Ends the lost process, waits for it to exit, then runs `start`.
///
/// A replacement that loads while the old process still holds GPU memory can fail
/// its CUDA load and fall back to a slower backend, so the exit is awaited first.
/// Returns whether the engine came back.
pub(super) async fn restart_engine_after_exit<F, Fut>(
    previous: Option<FunasrProcess>,
    start: F,
) -> bool
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), AppError>>,
{
    if let Some(mut process) = previous {
        let _ = process.child.start_kill();
        if tokio::time::timeout(LOST_PROCESS_EXIT_BUDGET, process.child.wait())
            .await
            .is_err()
        {
            log::warn!(
                "Lost FunASR process did not exit within {}s; restarting anyway",
                LOST_PROCESS_EXIT_BUDGET.as_secs()
            );
        }
    }
    match start().await {
        Ok(()) => {
            log::info!("FunASR engine restarted after its process was lost");
            true
        }
        Err(err) => {
            log::warn!("FunASR engine restart after a lost process failed: {err}");
            false
        }
    }
}

/// Restarts the local engine in the background after its process was lost mid-session.
///
/// Frontend polling stops once the engine reports ready, so a process that dies during
/// a dictation used to stay down until the app restarted. `start_server` ignores the
/// request while another start runs or a switch has blocked starts, so repeats are safe.
pub(super) fn request_engine_recovery(previous: Option<FunasrProcess>) {
    let Some(app_handle) = ENGINE_RECOVERY_APP.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        if paths::is_online_engine(&paths::read_engine_config()) {
            return;
        }
        let state = app_handle.state::<AppState>();
        restart_engine_after_exit(previous, || start_server(&app_handle, state.inner())).await;
    });
}

/// Start the FunASR Python server
pub async fn start_server(app_handle: &tauri::AppHandle, state: &AppState) -> Result<(), AppError> {
    let (ticket, mut starting_guard) = {
        // Startup preflight is serialized with lifecycle config changes: the engine snapshot, owner, generation
        // and the first loading state must be committed as one indivisible unit.
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            return Ok(());
        }

        let process_guard = state.engine.funasr_process.lock().await;
        if process_guard.is_some() {
            log::warn!("FunASR server is already running");
            return Ok(());
        }
        drop(process_guard);
        if state.engine.funasr_starting_process.lock().is_some() {
            log::info!("FunASR child process is initializing; skipping duplicate start");
            return Ok(());
        }

        let start_owner = next_server_start_owner();
        if !state.engine.try_begin_funasr_start(start_owner) {
            log::info!("FunASR server is starting; skipping duplicate start");
            return Ok(());
        }

        let ticket = StartTicket {
            owner: start_owner,
            generation: state.engine.funasr_generation.load(Ordering::SeqCst),
            engine,
        };
        let starting_guard = StartingFlagGuard {
            engine: &state.engine,
            owner: start_owner,
            child: None,
        };

        state.set_funasr_ready(false);
        state.set_inline_audio_transport(None);
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "Locating engine runtime..."
            }),
        );
        (ticket, starting_guard)
    };

    // Locate the engine runtime
    let runtime_result = find_engine(app_handle, state, ticket.generation).await;
    {
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!("Engine runtime is ready but the start ticket is no longer valid; cancelling this start");
            return Err(AppError::Asr("Startup was cancelled".to_string()));
        }
    }
    let runtime = runtime_result?;

    if !start_ticket_is_current(state, &ticket) {
        log::warn!("Engine runtime is ready but the generation has changed; cancelling this start");
        return Err(AppError::Asr("Startup was cancelled".to_string()));
    }

    // Build the child process command
    let data_dir = paths::strip_win_prefix(paths::get_data_dir());
    // Passed only when the user enables it. 0/default sets no environment variable, so the startup path is the same as before.
    let gpu_idle_at_spawn = paths::read_gpu_idle_seconds();
    let model_dirs_at_spawn = paths::read_model_dirs();
    let mut cmd = match &runtime {
        EngineRuntime::Bundled { exe_path } => {
            log::info!(
                "Using bundled engine: {} (engine={})",
                exe_path,
                ticket.engine
            );
            let mut c = Command::new(exe_path);
            c.arg("serve").arg("--engine").arg(&ticket.engine);
            c
        }
        EngineRuntime::Development { python_path } => {
            log::info!("Using development-mode Python: {}", python_path);
            let server_script = match ticket.engine.as_str() {
                "confucius4-r2t2" => paths::get_r2t2_asr_server_path(app_handle),
                engine if paths::is_transcribe_cpp_engine(engine) => {
                    paths::get_qwen3_asr_server_path(app_handle)
                }
                engine => {
                    return Err(AppError::Asr(format!(
                        "Unsupported local ASR engine: {engine}"
                    )))
                }
            };
            let server_script_str = paths::strip_win_prefix(&server_script);
            log::info!(
                "Speech recognition script path (engine={}): {}",
                ticket.engine,
                server_script_str
            );

            if !server_script.exists() {
                return Err(AppError::Asr(format!(
                    "FunASR server script does not exist: {}",
                    server_script_str
                )));
            }

            let mut c = Command::new(python_path);
            c.arg("-X").arg("utf8").arg("-u").arg(&server_script_str);
            c
        }
    };

    let models_dir = paths::strip_win_prefix(&paths::get_effective_models_dir());
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("SPEAK_FLOW_DATA_DIR", &data_dir)
        .env("SPEAK_FLOW_ASR_ENGINE", &ticket.engine)
        .env("HF_HUB_CACHE", &models_dir)
        // Read-only folders the user added. The engine looks there only when the
        // cache has no complete copy, and never writes to them.
        .env("SPEAK_FLOW_MODEL_DIRS", model_dirs_at_spawn.join(";"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr({
            let log_path = paths::get_data_dir().join("funasr_stderr.log");
            match std::fs::File::create(&log_path) {
                Ok(file) => {
                    log::info!("Python stderr redirected to: {}", log_path.display());
                    std::process::Stdio::from(file)
                }
                Err(e) => {
                    log::warn!(
                        "Could not create the stderr log file: {}; discarding stderr",
                        e
                    );
                    std::process::Stdio::null()
                }
            }
        });

    if gpu_idle_at_spawn > 0 {
        cmd.env("SPEAK_FLOW_GPU_IDLE_SECONDS", gpu_idle_at_spawn.to_string());
    }
    if ticket.engine.starts_with("custom:") {
        let custom = crate::services::custom_models::resolve_custom_model(&ticket.engine)
            .ok_or_else(|| {
                AppError::Asr(format!("Custom model {} is not registered", ticket.engine))
            })?;
        if !std::path::Path::new(&custom.path).is_file() {
            return Err(AppError::Asr(format!(
                "Custom model file missing on disk: {}",
                custom.path
            )));
        }
        cmd.env("SPEAK_FLOW_CUSTOM_MODEL_PATH", &custom.path);
        cmd.env("SPEAK_FLOW_CUSTOM_MODEL_NAME", &custom.name);
    }

    // Hide the console window on Windows
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let (stdin, stdout, child_handle) = {
        // Spawn and starting-slot registration share one lifecycle critical section, so stop cannot miss
        // a child process that is "spawned but not yet written into the official process slot".
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            return Err(AppError::Asr("Startup was cancelled".to_string()));
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Asr(format!("Failed to start FunASR process: {}", e)))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            let _ = child.start_kill();
            AppError::Asr("Failed to capture FunASR process stdin".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            let _ = child.start_kill();
            AppError::Asr("Failed to capture FunASR process stdout".to_string())
        })?;
        let child_handle = Arc::new(parking_lot::Mutex::new(Some(child)));
        *state.engine.funasr_starting_process.lock() = Some(StartingFunasrProcess {
            owner: ticket.owner,
            generation: ticket.generation,
            child: child_handle.clone(),
        });
        starting_guard.register_child(child_handle.clone());

        log::info!("FunASR child process started; waiting for initialization...");
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "Loading speech recognition model..."
            }),
        );
        (stdin, stdout, child_handle)
    };

    // Read the child's initialization output, skipping non-JSON lines until a valid response arrives
    let mut stdout_reader = BufReader::new(stdout);
    let response: ServerResponse = match read_json_response(
        &mut stdout_reader,
        Duration::from_secs(SERVER_INIT_TIMEOUT_SECS),
        "FunASR initialization",
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            return if start_ticket_is_current(state, &ticket) {
                Err(err)
            } else {
                Err(AppError::Asr("Startup was cancelled".to_string()))
            };
        }
    };

    let model_loaded = response.is_model_loaded();
    let initialized = response.initialized.unwrap_or(false)
        || response.success.unwrap_or(false)
        || response.status.as_deref() == Some("ready")
        || model_loaded;

    let error_message = response
        .error
        .clone()
        .or_else(|| response.message.clone())
        .unwrap_or_else(|| "FunASR initialization failed".to_string());

    if initialized {
        let ready_status = serde_json::json!({
            "status": "ready",
            "message": "FunASR server is ready",
            "device": response.device,
            "gpu_name": response.gpu_name,
            "models_present": true,
            "missing_models": [],
            "model_state": "ready",
            "corrupt_models": [],
        });
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!("FunASR initialization finished but the start ticket is no longer valid; discarding the old process");
            return Err(AppError::Asr("Startup was cancelled".to_string()));
        }

        let Some(mut child) = take_starting_child(state, &ticket, &child_handle) else {
            return Err(AppError::Asr("Startup was cancelled".to_string()));
        };
        let mut process_guard = state.engine.funasr_process.lock().await;
        if process_guard.is_some() {
            let _ = child.start_kill();
            return Err(AppError::Asr(
                "FunASR server was already started by another task".to_string(),
            ));
        }
        *process_guard = Some(FunasrProcess {
            child,
            stdin,
            stdout: stdout_reader,
        });
        starting_guard.release_child();
        drop(process_guard);
        log::info!("FunASR server initialized successfully!");
        // A folder saved while the process was starting is not in its
        // environment. This must succeed before the server is announced ready:
        // otherwise the UI stops polling and dictation fails with no recovery.
        if paths::read_model_dirs() != model_dirs_at_spawn {
            if let Err(error) = super::push_model_dirs(state).await {
                log::error!("Failed to sync the model folder; aborting start: {error}");
                let _ = super::stop_server(state).await;
                state.set_funasr_ready(false);
                let _ = app_handle.emit(
                    "funasr-status",
                    serde_json::json!({
                        "status": "error",
                        "message": format!("Could not apply the model folders: {error}")
                    }),
                );
                return Err(AppError::Asr(format!(
                    "Could not apply the model folders: {error}"
                )));
            }
        }
        let supports_streaming = response
            .supports_streaming
            .unwrap_or(ticket.engine == paths::R2T2_ENGINE);
        let supports_diarization = response.supports_diarization.unwrap_or(false);
        state
            .engine
            .supports_native_streaming
            .store(supports_streaming, Ordering::Release);
        state
            .engine
            .supports_diarization
            .store(supports_diarization, Ordering::Release);
        state.set_funasr_ready(true);
        let _ = app_handle.emit("funasr-status", ready_status);
    } else {
        log::error!("FunASR initialization failed: {}", error_message);
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!(
                "FunASR initialization failure result is stale; ignoring the old error state"
            );
            return Err(AppError::Asr("Startup was cancelled".to_string()));
        }
        let Some(mut child) = take_starting_child(state, &ticket, &child_handle) else {
            return Err(AppError::Asr("Startup was cancelled".to_string()));
        };
        let _ = child.start_kill();
        starting_guard.release_child();
        state.set_funasr_ready(false);
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "error",
                "message": &error_message
            }),
        );
    }

    if initialized {
        let gpu_idle_now = paths::read_gpu_idle_seconds();
        // The environment variable at startup already carried the value of that moment. Send another command only if it changed midway,
        // avoiding an extra round trip when it is off.
        if gpu_idle_now != gpu_idle_at_spawn {
            if let Err(error) = push_gpu_idle_seconds(state, gpu_idle_now).await {
                log::warn!("Failed to sync the GPU idle-unload setting: {error}");
            }
        }

        Ok(())
    } else {
        Err(AppError::Asr(error_message))
    }
}

/// Run speech transcription
///
/// Writes the audio data to a temporary WAV file, then sends a transcribe command to the Python process via stdin
/// and reads the result from stdout.
///
/// # Parameters
/// - `state`: global application state
/// - `audio_data`: audio data in WAV format (byte array)
///
/// # Flow
///
/// Sends a status command to the Python server to get its current state.
pub async fn check_status(
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<FunASRStatus, AppError> {
    // First check whether the process exists
    let has_process = {
        let guard = state.engine.funasr_process.lock().await;
        guard.is_some()
    };

    // If there is no process handle, check whether it is starting
    if !has_process {
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            let has_key = !state.read_online_asr_api_key().is_empty();
            let label = match engine.as_str() {
                "alibaba-asr" => "Alibaba DashScope",
                _ => "GLM-ASR",
            };
            return Ok(FunASRStatus {
                running: true,
                ready: has_key,
                model_loaded: true,
                device: Some("cloud".into()),
                gpu_name: None,
                gpu_memory_total: None,
                message: if has_key {
                    format!("{} online service ready", label)
                } else {
                    format!("Please configure the {} API Key", label)
                },
                engine: Some(engine),
                models_present: Some(true),
                missing_models: Some(Vec::new()),
                model_state: if has_key {
                    Some(ModelState::Ready)
                } else {
                    None
                },
                corrupt_models: Some(Vec::new()),
            });
        }

        if state.engine.is_funasr_starting() {
            // Starting (model loading): tell the frontend "running but not ready yet"
            return Ok(status_with_defaults(
                true,
                false,
                false,
                "FunASR server is starting and loading models...".to_string(),
            ));
        }
        let model_check = inspect_model_files_for_engine(&engine);
        let has_engine = paths::get_engine_exe_path(app_handle).is_some()
            || paths::get_engine_archive_path(app_handle).is_some()
            || (cfg!(debug_assertions)
                && (paths::get_resource_engine_exe_path(app_handle).is_some()
                    || installation::find_python().await.is_ok()));
        let all_present = model_check.all_present && has_engine;
        let model_state = model_state_with_engine(model_check.model_state, has_engine);
        return Ok(FunASRStatus {
            message: if !has_engine && model_state != ModelState::Corrupt {
                "Speech engine is not downloaded; please download the models and engine first"
                    .to_string()
            } else {
                match model_state {
                    ModelState::Ready => "FunASR server is not running".to_string(),
                    ModelState::Missing => {
                        "Model files have not been downloaded; download the models first"
                            .to_string()
                    }
                    ModelState::Corrupt => {
                        "Model files failed validation; please repair the models".to_string()
                    }
                }
            },
            engine: Some(engine),
            models_present: Some(all_present),
            missing_models: Some(model_check.missing_models.clone()),
            model_state: Some(model_state),
            corrupt_models: Some(model_check.corrupt_models.clone()),
            ..status_with_defaults(false, false, false, String::new())
        });
    }

    // Send the status query command
    match send_command_to_server(state, &ServerCommand::Status, Some(app_handle)).await {
        Ok(response) => {
            let model_loaded = response.is_model_loaded();

            let initialized = response.initialized.unwrap_or(false) || model_loaded;
            if initialized {
                state.set_funasr_ready(true);
                if let Some(s) = response.supports_streaming {
                    state
                        .engine
                        .supports_native_streaming
                        .store(s, Ordering::Release);
                }
                if let Some(d) = response.supports_diarization {
                    state
                        .engine
                        .supports_diarization
                        .store(d, Ordering::Release);
                }
            }

            let ready = state.is_funasr_ready() || initialized;
            let message = response
                .message
                .or(response.error)
                .unwrap_or_else(|| "Server is running".to_string());

            Ok(FunASRStatus {
                running: true,
                ready,
                model_loaded,
                device: response.device,
                gpu_name: response.gpu_name,
                gpu_memory_total: response.gpu_memory_total,
                message,
                engine: response.engine,
                models_present: Some(true),
                missing_models: Some(Vec::new()),
                model_state: Some(ModelState::Ready),
                corrupt_models: Some(Vec::new()),
            })
        }
        Err(e) => {
            // Sending the command failed; the process may have crashed
            log::warn!("Failed to query FunASR status: {}", e);
            state.set_funasr_ready(false);
            Ok(status_with_defaults(
                false,
                false,
                false,
                format!("Server communication failed: {}", e),
            ))
        }
    }
}

/// Stop the FunASR server
///
/// Graceful shutdown flow:
/// 1. Send an exit command first so the Python process quits on its own
/// 2. Wait a short while
/// 3. If the process is still running, kill it
///
pub async fn stop_server(state: &AppState) -> Result<(), AppError> {
    // Bump the generation so an in-flight start_server notices the cancellation
    {
        let _status_guard = state.engine.funasr_status_commit.lock();
        state
            .engine
            .funasr_generation
            .fetch_add(1, Ordering::SeqCst);
    }

    // A child spawned but not yet finished initializing must also be cancellable immediately; otherwise, after set_engine
    // returns, the old model process could still load in the background and keep sending status.
    if let Some(starting) = state.engine.funasr_starting_process.lock().take() {
        if let Some(mut child) = starting.child.lock().take() {
            let _ = child.start_kill();
        }
    }

    // Take the child process handle out first so shutdown is not held up by the regular request timeout
    let mut process = {
        let mut guard = state.engine.funasr_process.lock().await;
        guard.take()
    };

    // If there is a child process, make sure it is terminated
    if let Some(ref mut child_process) = process {
        if let Err(err) = try_send_exit_command(child_process).await {
            log::debug!("Failed to send the FunASR exit command; about to wait for / terminate the process directly: {}", err);
        }

        let wait_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(SERVER_EXIT_WAIT_TIMEOUT_SECS),
            child_process.child.wait(),
        )
        .await;

        match wait_result {
            Ok(Ok(status)) => {
                log::info!("FunASR process exited with status: {}", status);
            }
            _ => {
                // Timed out or errored; kill the process
                log::warn!(
                    "FunASR process did not respond to the exit command; forcing termination..."
                );
                if let Err(e) = child_process.child.kill().await {
                    log::warn!("Failed to force-terminate the FunASR process: {}", e);
                }
            }
        }
    }

    // Update state
    state.set_funasr_ready(false);
    state
        .engine
        .supports_native_streaming
        .store(false, Ordering::Release);
    state
        .engine
        .supports_diarization
        .store(false, Ordering::Release);
    state.set_inline_audio_transport(None);

    log::info!("FunASR server stopped");
    Ok(())
}
