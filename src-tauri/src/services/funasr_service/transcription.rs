use super::*;

/// ```text
/// 音频数据 -> 临时文件 -> 发送命令给 Python -> 等待结果 -> 返回文本
/// ```
pub async fn transcribe(
    state: &AppState,
    audio_data: Vec<u8>,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    let _asr_guard = state.engine.native_asr_owner.lock().await;
    let hot_words = profile_hot_words(state);
    transcribe_wav_bytes_via_path(
        state,
        audio_data,
        hot_words,
        app_handle,
        TranscribeOptions { final_pass: true },
    )
    .await
}

/// Which pass a request belongs to. Filler removal only touches the finished
/// transcript, so live previews never flicker as words disappear.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranscribeOptions {
    pub final_pass: bool,
}

pub async fn transcribe_pcm16(
    state: &AppState,
    samples: &[i16],
    sample_rate: u32,
    app_handle: &tauri::AppHandle,
    options: TranscribeOptions,
) -> Result<TranscriptionResult, AppError> {
    let _asr_guard = state.engine.native_asr_owner.lock().await;
    transcribe_pcm16_owned(state, samples, sample_rate, app_handle, options).await
}

/// Same as [`transcribe_pcm16`] for a caller that already owns the engine.
///
/// The model test holds `native_asr_owner` across its recording and decode, so
/// it must not take the lock again.
pub async fn transcribe_pcm16_owned(
    state: &AppState,
    samples: &[i16],
    sample_rate: u32,
    app_handle: &tauri::AppHandle,
    options: TranscribeOptions,
) -> Result<TranscriptionResult, AppError> {
    // 检查服务器是否就绪
    if !state.is_funasr_ready() {
        return Err(AppError::Asr(
            "Speech recognition engine is not ready yet; please wait for initialization to finish"
                .to_string(),
        ));
    }

    // 本地引擎对过短输入的处理不稳定。interim 路径的首个 tick 可能只有
    // 200-400ms，这里尾部 zero-pad 到 0.5s；只对短音频生效，长输入走原路。
    const MIN_IPC_DURATION_SEC: f64 = 0.5;
    let min_samples_for_ipc = (sample_rate as f64 * MIN_IPC_DURATION_SEC) as usize;
    let padded_storage: Vec<i16>;
    let samples: &[i16] = if paths::read_engine_config() != "confucius4-r2t2"
        && samples.len() < min_samples_for_ipc
        && !samples.is_empty()
    {
        let mut v = Vec::with_capacity(min_samples_for_ipc);
        v.extend_from_slice(samples);
        v.resize(min_samples_for_ipc, 0);
        padded_storage = v;
        &padded_storage
    } else {
        samples
    };

    let hot_words = profile_hot_words(state);

    // The Python memory protocol receives raw samples only. It uses sample_rate
    // for duration accounting, while the ASR backends assume the in-memory array
    // is already at their expected rate. Preserve non-16k audio by sending WAV.
    if sample_rate != TARGET_SAMPLE_RATE_FOR_INLINE_AUDIO {
        return transcribe_pcm16_via_path(
            state,
            samples,
            sample_rate,
            hot_words,
            app_handle,
            options,
        )
        .await;
    }

    if state.inline_audio_transport() == Some(false) {
        return transcribe_pcm16_via_path(
            state,
            samples,
            sample_rate,
            hot_words,
            app_handle,
            options,
        )
        .await;
    }

    let (language, translate_to_english) = transcription_options();
    let response = send_command_to_server(
        state,
        &ServerCommand::Transcribe {
            options: profile_r2t2_options(state),
            audio_path: None,
            audio_base64: Some(encode_pcm16_base64(samples)),
            audio_format: Some(INLINE_AUDIO_FORMAT_PCM_S16LE.to_string()),
            sample_rate: Some(sample_rate),
            hot_words: hot_words.clone(),
            language,
            translate_to_english,
            filter_silence: Some(paths::read_silence_filter()),
        },
        Some(app_handle),
    )
    .await?;

    if response.input_mode.as_deref() == Some("memory") {
        state.set_inline_audio_transport(Some(true));
        return Ok(server_response_to_transcription_result(
            state, response, options,
        ));
    }

    if response_indicates_inline_unsupported(&response) {
        log::info!("当前 FunASR 运行时不支持内存音频，回退到临时 WAV 文件");
        state.set_inline_audio_transport(Some(false));
        return transcribe_pcm16_via_path(
            state,
            samples,
            sample_rate,
            hot_words,
            app_handle,
            options,
        )
        .await;
    }

    state.set_inline_audio_transport(Some(true));
    Ok(server_response_to_transcription_result(
        state, response, options,
    ))
}

pub(super) fn profile_hot_words(state: &AppState) -> Option<Vec<String>> {
    let words = state.with_profile(|p| p.get_hot_word_texts(100));
    (!words.is_empty()).then_some(words)
}

/// Recognition language and translation for the local transcribe.cpp models.
///
/// R2T2 carries its language in `options`, and online engines never reach this
/// command. The saved language is resolved once against the selected model, so
/// a switch to an English-only model never sends an unusable code.
fn transcription_options() -> (Option<String>, Option<bool>) {
    let engine = paths::read_engine_config();
    if engine == paths::R2T2_ENGINE || paths::is_online_engine(&engine) {
        return (None, None);
    }
    let language =
        crate::utils::asr_catalog::resolve_language(&engine, paths::read_asr_language().as_deref());
    let translate = paths::read_translate_to_english()
        && crate::utils::asr_catalog::supports_translation(&engine)
        && language != "en";
    (
        (language != "auto").then_some(language),
        translate.then_some(true),
    )
}

pub(super) fn profile_r2t2_options(
    state: &AppState,
) -> Option<crate::state::user_profile::R2T2Config> {
    (paths::read_engine_config() == "confucius4-r2t2")
        .then(|| state.with_profile(|profile| profile.r2t2.clone()))
}

const _: () = assert!(
    cfg!(target_endian = "little"),
    "encode_pcm16_base64 reinterprets i16 memory as little-endian PCM bytes"
);

pub(super) fn encode_pcm16_base64(samples: &[i16]) -> String {
    use base64::Engine;

    // 零拷贝 cast：i16 对齐 2、无填充，pcm_s16le 在 LE 目标上（Windows/macOS/Linux
    // 的 x86/ARM）就是 i16 的内存布局。避免原先逐样本 extend_from_slice 造成的
    // 192000 次小拷贝 + 一次 384KB Vec 分配，interim loop 每 ~220ms 一次的热点。
    let byte_slice: &[u8] =
        // SAFETY: i16 samples are contiguous and initialized, len * 2 is their exact byte length, and the slice borrows samples for its lifetime.
        unsafe { std::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), samples.len() * 2) };
    base64::engine::general_purpose::STANDARD.encode(byte_slice)
}

pub(super) fn encode_wav_bytes(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, AppError> {
    crate::services::audio_service::encode_wav(samples, sample_rate)
}

pub(super) fn create_temp_audio_path() -> std::path::PathBuf {
    static TEMP_AUDIO_COUNTER: AtomicU64 = AtomicU64::new(1);
    let counter = TEMP_AUDIO_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "speak_flow_audio_{}_{}_{}.wav",
        std::process::id(),
        now_unix_ms(),
        counter,
    ))
}

pub(super) fn response_indicates_inline_unsupported(response: &ServerResponse) -> bool {
    if response.input_mode.as_deref() == Some("memory") {
        return false;
    }

    if response.input_mode.is_none() || response.input_mode.as_deref() == Some("path") {
        return true;
    }

    response.error.as_deref().is_some_and(|error| {
        error.contains("Audio file does not exist")
            || error.contains("音频文件不存在")
            || error.contains("path should be string")
            || error.contains("os.PathLike")
            || error.contains("NoneType")
    })
}

/// Applies the final text steps shared by every path that produces a transcript.
///
/// The server response and the finalize path that reuses an interim result both
/// call this, so a reused transcript is cleaned exactly like a fresh one.
pub fn finish_text(
    state: &AppState,
    text: &str,
    language: Option<&str>,
    options: TranscribeOptions,
) -> String {
    let corrected =
        crate::services::qwen_hotword_service::apply_profile_hotword_corrections(state, text);
    if !options.final_pass {
        return corrected;
    }
    apply_filler_removal(state, &corrected, language)
}

/// Removes fillers from text that already went through corrections.
///
/// The finalize path reuses an interim transcript that was corrected when it was
/// produced, so it must not correct it a second time.
pub fn apply_filler_removal(state: &AppState, text: &str, language: Option<&str>) -> String {
    if !crate::services::filler_words::should_remove_fillers(language, paths::read_remove_fillers())
    {
        return text.to_string();
    }
    // Every dictionary entry is protected, not just the ones sent as recognition
    // hints, so a word the user added on purpose survives even when it looks
    // like a filler.
    let protected = state.with_profile(|profile| {
        profile
            .hot_words
            .iter()
            .map(|word| word.text.clone())
            .collect::<Vec<_>>()
    });
    crate::services::filler_words::remove_fillers(text, &protected)
}

pub(super) fn server_response_to_transcription_result(
    state: &AppState,
    response: ServerResponse,
    options: TranscribeOptions,
) -> TranscriptionResult {
    if response.success == Some(true) {
        let text = finish_text(
            state,
            response.text.as_deref().unwrap_or_default(),
            response.language.as_deref(),
            options,
        );

        TranscriptionResult {
            text,
            duration: response.duration,
            success: true,
            error: None,
            language: response.language,
        }
    } else {
        let error_msg = response
            .error
            .unwrap_or_else(|| "Unknown transcription error".to_string());
        TranscriptionResult {
            text: String::new(),
            duration: None,
            success: false,
            error: Some(error_msg),
            language: None,
        }
    }
}

pub(super) async fn transcribe_wav_bytes_via_path(
    state: &AppState,
    audio_data: Vec<u8>,
    hot_words: Option<Vec<String>>,
    app_handle: &tauri::AppHandle,
    options: TranscribeOptions,
) -> Result<TranscriptionResult, AppError> {
    let temp_file = create_temp_audio_path();

    tokio::fs::write(&temp_file, &audio_data)
        .await
        .map_err(|e| AppError::Asr(format!("Failed to write temporary audio file: {}", e)))?;

    let (language, translate_to_english) = transcription_options();
    let response = send_command_to_server(
        state,
        &ServerCommand::Transcribe {
            options: profile_r2t2_options(state),
            audio_path: Some(temp_file.to_string_lossy().to_string()),
            audio_base64: None,
            audio_format: None,
            sample_rate: None,
            hot_words,
            language,
            translate_to_english,
            filter_silence: Some(paths::read_silence_filter()),
        },
        Some(app_handle),
    )
    .await;

    let _ = tokio::fs::remove_file(&temp_file).await;
    response.map(|response| server_response_to_transcription_result(state, response, options))
}

pub(super) async fn transcribe_pcm16_via_path(
    state: &AppState,
    samples: &[i16],
    sample_rate: u32,
    hot_words: Option<Vec<String>>,
    app_handle: &tauri::AppHandle,
    options: TranscribeOptions,
) -> Result<TranscriptionResult, AppError> {
    let wav_bytes = encode_wav_bytes(samples, sample_rate)?;
    transcribe_wav_bytes_via_path(state, wav_bytes, hot_words, app_handle, options).await
}
