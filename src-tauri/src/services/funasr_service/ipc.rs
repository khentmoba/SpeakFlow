use super::*;

/// 向 Python 服务器发送命令并读取响应
///
/// 这是与 Python 进程通信的核心函数。
///
/// # 通信流程
/// 1. 持有进程锁，从全局状态中取出子进程
/// 2. 将命令序列化为 JSON
/// 3. 通过 stdin 写入 JSON + 换行符
/// 4. 从 stdout 读取一行 JSON 响应
/// 5. 把子进程放回全局状态
/// 6. 反序列化响应并返回
///
/// # 注意事项
/// - 每条消息必须以换行符结尾
/// - 命令和响应都是单行 JSON
/// - 为了保证同一时间只有一个命令与子进程通信，
///   这里会在 I/O 完成前保持锁，避免并发读写导致协议错乱。
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
        if self.process.is_none() {
            return;
        }
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

/// 向子进程发送命令并读取响应的内部实现
///
/// 调用方在整个 I/O 期间持有进程锁和取消守卫。
pub(super) async fn send_command_impl(
    process: &mut FunasrProcess,
    command: &ServerCommand,
) -> Result<ServerResponse, AppError> {
    let request_id = NEXT_SERVER_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    // 直接单次序列化为字节流，避免先 to_value 深拷贝 512KB base64 负载再 to_string 再 format
    let mut command_bytes = serde_json::to_vec(&CommandEnvelope {
        request_id,
        command,
    })
    .map_err(|e| AppError::Asr(format!("Failed to serialize command: {}", e)))?;
    command_bytes.push(b'\n');

    // 写入命令到 stdin
    // `write_all` 确保所有字节都被写入
    process
        .stdin
        .write_all(&command_bytes)
        .await
        .map_err(|e| AppError::Asr(format!("Failed to write command to FunASR: {}", e)))?;

    // `flush` 确保缓冲区的数据被立即发送
    process
        .stdin
        .flush()
        .await
        .map_err(|e| AppError::Asr(format!("Failed to flush stdin buffer: {}", e)))?;

    // 从 stdout 读取响应（允许跳过非 JSON 行）
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
