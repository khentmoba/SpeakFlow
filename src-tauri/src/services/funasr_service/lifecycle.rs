use super::*;

/// 启动 FunASR Python 服务器
pub async fn start_server(app_handle: &tauri::AppHandle, state: &AppState) -> Result<(), AppError> {
    let (ticket, mut starting_guard) = {
        // 启动预检与 lifecycle 配置变更串行：engine snapshot、owner、generation
        // 以及首个 loading 状态必须作为一个不可分割的提交。
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            return Ok(());
        }

        let process_guard = state.engine.funasr_process.lock().await;
        if process_guard.is_some() {
            log::warn!("FunASR 服务器已在运行中");
            return Ok(());
        }
        drop(process_guard);
        if state.engine.funasr_starting_process.lock().is_some() {
            log::info!("FunASR 子进程正在初始化中，跳过重复启动");
            return Ok(());
        }

        let start_owner = next_server_start_owner();
        if !state.engine.try_begin_funasr_start(start_owner) {
            log::info!("FunASR 服务器正在启动中，跳过重复启动");
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

    // 查找引擎运行时
    let runtime_result = find_engine(app_handle, state, ticket.generation).await;
    {
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!("引擎运行时已就绪但启动票据已失效，取消本次启动");
            return Err(AppError::Asr("Startup was cancelled".to_string()));
        }
    }
    let runtime = runtime_result?;

    if !start_ticket_is_current(state, &ticket) {
        log::warn!("引擎运行时已就绪但代数已变更，取消本次启动");
        return Err(AppError::Asr("Startup was cancelled".to_string()));
    }

    // 构建子进程命令
    let data_dir = paths::strip_win_prefix(paths::get_data_dir());
    // 仅在用户开启时传入。0/缺省不设置环境变量，启动路径与以前相同。
    let gpu_idle_at_spawn = paths::read_gpu_idle_seconds();
    let mut cmd = match &runtime {
        EngineRuntime::Bundled { exe_path } => {
            log::info!("使用打包引擎: {} (engine={})", exe_path, ticket.engine);
            let mut c = Command::new(exe_path);
            c.arg("serve").arg("--engine").arg(&ticket.engine);
            c
        }
        EngineRuntime::Development { python_path } => {
            log::info!("使用开发模式 Python: {}", python_path);
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
                "语音识别脚本路径 (engine={}): {}",
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
        .env("SPEAK_FLOW_MODEL_DIRS", paths::read_model_dirs().join(";"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr({
            let log_path = paths::get_data_dir().join("funasr_stderr.log");
            match std::fs::File::create(&log_path) {
                Ok(file) => {
                    log::info!("Python stderr 重定向到: {}", log_path.display());
                    std::process::Stdio::from(file)
                }
                Err(e) => {
                    log::warn!("无法创建 stderr 日志文件: {}，丢弃 stderr", e);
                    std::process::Stdio::null()
                }
            }
        });

    if gpu_idle_at_spawn > 0 {
        cmd.env("SPEAK_FLOW_GPU_IDLE_SECONDS", gpu_idle_at_spawn.to_string());
    }

    // Windows 上隐藏控制台窗口
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let (stdin, stdout, child_handle) = {
        // spawn 与 starting slot 登记在同一 lifecycle 临界区，stop 不会漏掉
        // “已生成但尚未写入正式 process slot”的子进程。
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

        log::info!("FunASR 子进程已启动，等待初始化...");
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "Loading speech recognition model..."
            }),
        );
        (stdin, stdout, child_handle)
    };

    // 读取子进程初始化输出，跳过非 JSON 行，直到拿到有效响应
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
            log::warn!("FunASR 初始化完成但启动票据已失效，丢弃旧进程");
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
        log::info!("FunASR 服务器初始化成功！");
        state.set_funasr_ready(true);
        let _ = app_handle.emit("funasr-status", ready_status);
    } else {
        log::error!("FunASR 初始化失败: {}", error_message);
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!("FunASR 初始化失败结果已过期，忽略旧错误状态");
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
        // 启动时的环境变量已经带上当时的值。只有中途改过才再发一条命令，
        // 避免关闭状态下多一次往返。
        if gpu_idle_now != gpu_idle_at_spawn {
            if let Err(error) = push_gpu_idle_seconds(state, gpu_idle_now).await {
                log::warn!("同步 GPU 空闲卸载设置失败: {error}");
            }
        }
        Ok(())
    } else {
        Err(AppError::Asr(error_message))
    }
}

/// 执行语音转写
///
/// 将音频数据写入临时 WAV 文件，然后通过 stdin 发送转写命令给 Python 进程，
/// 并从 stdout 读取转写结果。
///
/// # 参数
/// - `state`：全局应用状态
/// - `audio_data`：WAV 格式的音频数据（字节数组）
///
/// # 流程
///
/// 发送 status 命令给 Python 服务器，获取当前的运行状态。
pub async fn check_status(
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<FunASRStatus, AppError> {
    // 先检查进程是否存在
    let has_process = {
        let guard = state.engine.funasr_process.lock().await;
        guard.is_some()
    };

    // 如果进程句柄不存在，检查是否正在启动中
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
            // 正在启动中（模型加载中），告诉前端"正在运行但还没准备好"
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

    // 发送状态查询命令
    match send_command_to_server(state, &ServerCommand::Status, Some(app_handle)).await {
        Ok(response) => {
            let model_loaded = response.is_model_loaded();

            let initialized = response.initialized.unwrap_or(false) || model_loaded;
            if initialized {
                state.set_funasr_ready(true);
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
            // 发送命令失败，可能进程已崩溃
            log::warn!("查询 FunASR 状态失败: {}", e);
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

/// 停止 FunASR 服务器
///
/// 优雅关闭流程：
/// 1. 先发送 exit 命令让 Python 进程自行退出
/// 2. 等待一小段时间
/// 3. 如果进程仍在运行，强制杀死
///
pub async fn stop_server(state: &AppState) -> Result<(), AppError> {
    // 递增代数，使正在进行的 start_server 感知到取消
    {
        let _status_guard = state.engine.funasr_status_commit.lock();
        state
            .engine
            .funasr_generation
            .fetch_add(1, Ordering::SeqCst);
    }

    // spawn 后、初始化完成前的子进程也必须立即可取消；否则 set_engine
    // 已返回时旧模型进程仍可能在后台加载并继续发送状态。
    if let Some(starting) = state.engine.funasr_starting_process.lock().take() {
        if let Some(mut child) = starting.child.lock().take() {
            let _ = child.start_kill();
        }
    }

    // 先取出子进程句柄，避免关闭流程被常规请求超时拖住
    let mut process = {
        let mut guard = state.engine.funasr_process.lock().await;
        guard.take()
    };

    // 如果有子进程，确保它被终止
    if let Some(ref mut child_process) = process {
        if let Err(err) = try_send_exit_command(child_process).await {
            log::debug!("发送 FunASR 退出命令失败，准备直接等待/终止进程: {}", err);
        }

        let wait_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(SERVER_EXIT_WAIT_TIMEOUT_SECS),
            child_process.child.wait(),
        )
        .await;

        match wait_result {
            Ok(Ok(status)) => {
                log::info!("FunASR 进程已退出，状态码: {}", status);
            }
            _ => {
                // 超时或出错，强制杀死进程
                log::warn!("FunASR 进程未响应退出命令，强制终止...");
                if let Err(e) = child_process.child.kill().await {
                    log::warn!("强制终止 FunASR 进程失败: {}", e);
                }
            }
        }
    }

    // 更新状态
    state.set_funasr_ready(false);
    state.set_inline_audio_transport(None);

    log::info!("FunASR 服务器已停止");
    Ok(())
}
