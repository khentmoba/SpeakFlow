mod commands;
mod ipc_policy;
mod services;
mod state;
mod tray_icon;
mod utils;

#[cfg(test)]
mod ipc_dto_contract_tests;

use state::{AppState, RecordingSlot};
use tauri::{Emitter, Manager};

pub fn cli_entry(args: Vec<String>) -> i32 {
    // `args` is the raw process argv, so drop the executable path before dispatch.
    services::cli_service::run_cli(args.into_iter().skip(1).collect())
}

pub fn run() {
    // Startup timing is logged so a regression in first-paint latency is measurable.
    let started = std::time::Instant::now();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let minimized = app
                .state::<AppState>()
                .with_profile(|profile| profile.autostart_minimized);
            if should_start_hidden(minimized, args.into_iter()) {
                return;
            }
            // When an instance is already running, focus the main window
            focus_main_window(app);
        }))
        .plugin(tauri_plugin_keyring::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("app".into()),
                    },
                ))
                .level(log::LevelFilter::Info)
                .max_file_size(5_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                .build(),
        )
        .manage(AppState::new())
        .on_page_load(move |webview, payload| {
            if webview.label() == "main"
                && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
            {
                log::info!(
                    "startup: main window page loaded in {}ms",
                    started.elapsed().as_millis()
                );
            }
        })
        .setup(move |app| {
            if !mark_setup_once() {
                log::warn!("setup() was called more than once; skipped");
                return Ok(());
            }

            let app_handle = app.handle().clone();
            services::funasr_service::register_engine_recovery_app(app_handle.clone());
            log::info!(
                "SpeakFlow app started, data directory: {:?}",
                utils::paths::get_data_dir()
            );

            // The local-only switch gates every outbound request, so it must be
            // in memory before any service can start one.
            services::local_only::load();

            // Load the user profile at startup
            {
                let state = app_handle.state::<AppState>();
                let loaded = services::profile_service::load_profile();
                // The main window starts hidden to avoid a flash during autostart.
                // A normal launch still opens it, regardless of this preference.
                if should_start_hidden(loaded.autostart_minimized, std::env::args()) {
                    log::info!("startup: main window stays hidden");
                } else {
                    log::info!("startup: main window shown");
                    focus_main_window(&app_handle);
                }
                // Migrate the legacy custom API key to the new keyring key (only when the target key does not exist)
                if loaded.llm_provider.active == "custom_migrated" {
                    use tauri_plugin_keyring::KeyringExt;
                    let new_user =
                        services::llm_provider::keyring_user_for_provider("custom_migrated");
                    let existing = app_handle
                        .keyring()
                        .get_password("speak-flow", &new_user)
                        .ok()
                        .flatten();
                    if existing.as_deref().unwrap_or("").is_empty() {
                        if let Some(old_key) = app_handle
                            .keyring()
                            .get_password("speak-flow", "custom-api-key")
                            .ok()
                            .flatten()
                            .filter(|k| !k.is_empty())
                        {
                            let _ = app_handle.keyring().set_password(
                                "speak-flow",
                                &new_user,
                                &old_key,
                            );
                            log::info!("Migrated custom API key to {}", new_user);
                        }
                    }
                }
                state.update_profile_mut(|profile| *profile = loaded.clone());
                services::audio_service::update_audio_behavior(loaded.audio_behavior.clone());
                services::audio_service::set_avoid_bluetooth_microphone(
                    loaded.audio_behavior.avoid_bluetooth_microphone,
                );
                let selected_device = state.selected_input_device_name();
                let audio_behavior = loaded.audio_behavior.clone();
                let labs = loaded.labs.clone();
                let preroll_app = app_handle.clone();
                // Opening the microphone waits for the stream to start, so keep it off the setup thread.
                tauri::async_runtime::spawn_blocking(move || {
                    // A hotkey press can reach stop_preroll_and_take before this task runs, so
                    // starting a ring now would prepend that recording's audio to the next dictation.
                    if preroll_app
                        .state::<AppState>()
                        .recording
                        .recording
                        .lock()
                        .is_some()
                    {
                        return;
                    }
                    services::audio_service::start_preroll_if_eligible(
                        selected_device,
                        &audio_behavior,
                        &labs,
                        false,
                    );
                });
                if let Err(err) = commands::hotkey::sync_extended_hotkeys(
                    &app_handle,
                    &loaded.shortcuts_behavior,
                    &loaded.labs,
                ) {
                    log::warn!("Failed to sync extended hotkeys at startup: {err}");
                }
                log::info!("User profile loaded");
            }

            {
                let app_handle = app_handle.clone();
                let mut statuses = services::profile_service::subscribe_profile_save_status();
                tauri::async_runtime::spawn(async move {
                    while statuses.changed().await.is_ok() {
                        let status = statuses.borrow_and_update().clone();
                        if let Err(error) =
                            app_handle.emit_to("main", "profile-save-status", status)
                        {
                            log::warn!("Failed to send the settings-saved status: {error}");
                        }
                    }
                });
            }

            {
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = services::history_service::initialize().await {
                        log::warn!("Failed to initialize local transcription history: {error}");
                        return;
                    }
                    let retention_days = app_handle
                        .state::<AppState>()
                        .with_profile(|profile| profile.history_settings.retention_days);
                    if let Err(error) = services::history_service::cleanup(retention_days).await {
                        log::warn!("Failed to clean up expired transcription history: {error}");
                    }
                });
            }

            // Register the translation/assistant hotkeys
            {
                type HotkeyRegisterFn =
                    dyn Fn(tauri::AppHandle, Option<String>) -> Result<String, utils::AppError>;
                let state = app_handle.state::<AppState>();
                let hotkeys: [(&str, &HotkeyRegisterFn, Option<String>); 2] = [
                    (
                        "translation",
                        &commands::hotkey::register_translation_hotkey_inner,
                        state.with_profile(|p| p.translation_hotkey.clone()),
                    ),
                    (
                        "assistant",
                        &commands::hotkey::register_assistant_hotkey_inner,
                        state.with_profile(|p| p.assistant_hotkey.clone()),
                    ),
                ];
                for (label, register, shortcut) in hotkeys {
                    if let Some(s) = shortcut.filter(|v| !v.trim().is_empty()) {
                        if let Err(err) = register(app_handle.clone(), Some(s)) {
                            log::warn!("Failed to register the {} hotkey: {}", label, err);
                        }
                    }
                }
            }

            // At startup, load API keys from the system keyring in the background to avoid blocking the main thread
            {
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    use tauri_plugin_keyring::KeyringExt;
                    let state = app_handle.state::<AppState>();
                    let provider = state.active_llm_provider();
                    let key =
                        services::llm_provider::load_api_key_for_provider(&app_handle, &provider);
                    if !key.is_empty() {
                        log::info!("Loaded the AI polish API key from the keyring (provider: {})", provider);
                    }
                    // Load the API key for the currently active online engine from the keyring (GLM / Alibaba-CN / Alibaba-Intl)
                    commands::funasr::reload_online_asr_key(&app_handle, state.inner());
                    if !state.read_online_asr_api_key().is_empty() {
                        log::info!(
                            "Loaded the online ASR API key from the keyring ({})",
                            commands::funasr::active_online_keyring_user()
                        );
                    }
                    // Load web search providers that need a key (keys are always stored in the system keyring).
                    for search_provider in [
                        crate::state::user_profile::WebSearchProvider::Exa,
                        crate::state::user_profile::WebSearchProvider::Tavily,
                        crate::state::user_profile::WebSearchProvider::Google,
                    ] {
                        let Some(keyring_user) =
                            commands::assistant::web_search_keyring_user(&search_provider)
                        else {
                            continue;
                        };
                        state.with_web_search_key_operation(|| {
                            if let Some(ws_key) = app_handle
                                .keyring()
                                .get_password("speak-flow", keyring_user)
                                .ok()
                                .flatten()
                                .filter(|key| !key.is_empty())
                            {
                                let cache_key = commands::assistant::web_search_provider_cache_key(
                                    &search_provider,
                                );
                                state.set_web_search_api_key(cache_key, ws_key);
                                log::info!("Loaded the web search API key from the keyring ({})", cache_key);
                            }
                        });
                    }

                    // Credentials from other tools are adopted only when the user
                    // asks for it (the Settings "Sync" action), never at startup.
                    if services::codex_oauth_service::sync_runtime_session(
                        &app_handle,
                        state.inner(),
                    )
                    .is_some()
                    {
                        log::info!("Loaded the OpenAI Codex OAuth session from the keyring");
                        spawn_openai_codex_oauth_prewarm(app_handle.clone());
                    }
                    if services::grok_build_oauth_service::sync_runtime_session(
                        &app_handle,
                        state.inner(),
                    )
                    .is_some()
                    {
                        log::info!("Loaded the Grok Build OAuth session from the keyring");
                        spawn_grok_build_oauth_prewarm(app_handle.clone());
                    }
                    if services::claude_oauth_service::sync_runtime_session(
                        &app_handle,
                        state.inner(),
                    )
                    .is_some()
                    {
                        log::info!("Loaded the Anthropic Claude OAuth session from the keyring");
                    }
                    spawn_funasr_startup(app_handle);
                });
            }
            spawn_profile_maintenance(app_handle.clone());
            // Labs > "Local API and CLI" is the opt-in for the loopback surface,
            // so the server only listens when the user turned it on.
            if app_handle
                .state::<AppState>()
                .with_profile(|profile| profile.labs.api_processing)
            {
                services::http_api_service::start_server(app_handle.clone());
            }
            spawn_meeting_call_monitor(app_handle.clone());
            if app_handle
                .state::<AppState>()
                .with_profile(|profile| profile.selection_assistant.enabled)
            {
                // Only users who enabled the assistant pay for its WebView at startup.
                // Otherwise the window is created on the first selection.
                if let Err(error) =
                    services::selection_service::create_selection_window(&app_handle)
                {
                    log::warn!("Selection assistant window pre-creation failed; will retry on first use: {error}");
                }
                if let Err(error) =
                    services::selection_service::start_selection_listener(app_handle.clone())
                {
                    log::error!("Failed to start the selection assistant listener: {error}");
                }
            }
            setup_system_tray(&app_handle)?;

            log::info!(
                "startup: setup completed in {}ms",
                started.elapsed().as_millis()
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    commands::window::trim_process_working_set();
                }
            }
        })
        .invoke_handler(ipc_policy::restrict_app_commands(tauri::generate_handler![
            commands::funasr::start_funasr,
            commands::funasr::transcribe_audio,
            commands::funasr::check_funasr_status,
            commands::funasr::check_model_files,
            commands::funasr::list_installed_models,
            commands::funasr::test_transcription,
            commands::funasr::get_voice_processing,
            commands::funasr::set_silence_filter,
            commands::funasr::set_remove_fillers,
            commands::dictation_tools::get_dictation_tools,
            commands::dictation_tools::set_dictation_tools,
            commands::file_transcription::transcribe_audio_file,
            commands::funasr::set_asr_language,
            commands::funasr::set_translate_to_english,
            commands::funasr::get_model_dirs,
            commands::funasr::set_model_dirs,
            commands::funasr::download_models,
            commands::funasr::cancel_model_download,
            commands::funasr::restart_funasr,
            commands::funasr::get_engine,
            commands::funasr::set_engine,
            commands::funasr::get_gpu_idle_seconds,
            commands::funasr::set_gpu_idle_seconds,
            commands::funasr::set_online_asr_api_key,
            commands::funasr::get_online_asr_api_key,
            commands::funasr::get_online_asr_endpoint,
            commands::funasr::set_online_asr_endpoint,
            commands::funasr::get_alibaba_asr_config,
            commands::funasr::set_alibaba_asr_model,
            commands::funasr::list_alibaba_asr_models,
            commands::funasr::get_models_dir,
            commands::funasr::set_models_dir,
            commands::funasr::pick_folder,
            commands::clipboard::copy_to_clipboard,
            commands::clipboard::paste_text,
            commands::clipboard::paste_last_dictation,
            commands::codex_oauth::login_openai_codex_oauth,
            commands::codex_oauth::start_openai_codex_oauth_device_code,
            commands::codex_oauth::complete_openai_codex_oauth_device_code,
            commands::codex_oauth::logout_openai_codex_oauth,
            commands::codex_oauth::remove_chatgpt_account,
            commands::codex_oauth::get_openai_codex_oauth_status,
            commands::codex_oauth::sync_openai_codex_oauth,
            commands::claude_oauth::start_claude_oauth,
            commands::claude_oauth::complete_claude_oauth,
            commands::claude_oauth::logout_claude_oauth,
            commands::claude_oauth::get_claude_oauth_status,
            commands::claude_oauth::sync_claude_oauth,
            commands::grok_build_oauth::login_grok_build_oauth,
            commands::grok_build_oauth::start_grok_build_oauth_device_code,
            commands::grok_build_oauth::complete_grok_build_oauth_device_code,
            commands::grok_build_oauth::logout_grok_build_oauth,
            commands::grok_build_oauth::get_grok_build_oauth_status,
            commands::window::hide_main_window,
            commands::profile::set_autostart_minimized,
            commands::window::show_subtitle_window,
            commands::window::hide_subtitle_window,
            commands::hotkey::register_custom_hotkey,
            commands::hotkey::register_translation_hotkey,
            commands::hotkey::register_assistant_hotkey,
            commands::hotkey::unregister_all_hotkeys,
            commands::hotkey::set_recording_mode,
            commands::hotkey::get_hotkey_diagnostic,
            commands::audio::start_recording,
            commands::audio::stop_recording,
            commands::audio::get_recording_snapshot,
            commands::audio::test_microphone,
            commands::audio::list_input_devices,
            commands::audio::set_input_device,
            commands::audio::start_microphone_level_monitor,
            commands::audio::stop_microphone_level_monitor,
            commands::audio::set_input_method,
            commands::audio::set_sound_enabled,
            commands::funasr::set_r2t2_config,
            commands::ai_polish::set_ai_polish_config,
            commands::ai_polish::get_ai_polish_config,
            commands::ai_polish::get_ai_polish_api_key,
            commands::ai_polish::set_ai_polish_api_key,
            commands::ai_polish::set_ai_polish_screen_context_enabled,
            commands::ai_polish::set_screen_context_enabled,
            commands::ai_polish::list_ai_models,
            commands::ai_polish::set_assistant_api_key,
            commands::ai_polish::get_assistant_api_key,
            commands::jev::set_jev_config,
            commands::jev::set_jev_provider,
            commands::jev::set_polish_mode,
            commands::magpie::detect_magpie,
            commands::jev::set_jev_features,
            commands::jev::set_screen_context_mode,
            commands::jev::set_web_search_mode,
            commands::jev::get_jev_api_key,
            commands::jev::set_jev_api_key,
            commands::profile::get_user_profile,
            commands::profile::get_profile_save_status,
            commands::profile::retry_profile_save,
            commands::profile::set_history_settings,
            commands::profile::set_app_profile_rules,
            commands::profile::add_hot_word,
            commands::profile::remove_hot_word,
            commands::profile::set_llm_provider_config,
            commands::profile::set_screen_vision_config,
            commands::profile::get_screen_vision_api_key,
            commands::profile::set_screen_vision_api_key,
            commands::profile::get_llm_reasoning_support,
            commands::profile::export_user_profile,
            commands::profile::import_user_profile,
            commands::profile::submit_user_correction,
            commands::profile::add_correction_rule,
            commands::profile::set_translation_target,
            commands::profile::set_translation_hotkey,
            commands::profile::set_custom_prompt,
            commands::profile::set_polish_structure_level,
            commands::profile::set_polish_reasoning_mode,
            commands::profile::set_openai_fast_mode,
            commands::profile::add_custom_provider,
            commands::profile::update_custom_provider,
            commands::profile::remove_custom_provider,
            commands::profile::validate_corrections,
            commands::profile::set_correction_validation_config,
            commands::profile::set_external_correction_learning,
            commands::profile::remove_correction,
            commands::updater::check_app_update,
            commands::updater::install_app_update,
            commands::updater::open_app_release_page,
            commands::assistant::set_assistant_hotkey,
            commands::assistant::set_assistant_system_prompt,
            commands::assistant::set_assistant_screen_context_enabled,
            commands::assistant::continue_assistant_conversation,
            commands::assistant::cancel_assistant_conversation,
            commands::assistant::retry_assistant_request,
            commands::assistant::open_assistant_source,
            commands::assistant::set_web_search_config,
            commands::assistant::set_web_search_api_key,
            commands::assistant::get_web_search_api_key,
            commands::selection::set_selection_assistant_config,
            commands::selection::set_selection_api_key,
            commands::selection::get_selection_api_key,
            commands::selection::resize_selection_window,
            commands::selection::hide_selection_assistant,
            commands::selection::start_selection_window_drag,
            commands::selection::get_selection_overlay_state,
            commands::selection::copy_selection,
            commands::selection::replace_selection,
            commands::selection::search_selection,
            commands::selection::run_selection_action,
            commands::selection::cancel_selection_action,
            commands::history::list_transcription_history,
            commands::history::get_transcription_history_stats,
            commands::history::search_transcripts,
            commands::history::delete_transcription_history,
            commands::history::export_transcription_history,
            commands::history::reprocess_transcription_history,
            commands::clipboard::paste_last_as_terminal,
            commands::hotkey::register_extended_hotkeys,
            commands::funasr::inspect_gguf_model,
            commands::funasr::list_custom_models,
            commands::funasr::add_custom_model,
            commands::funasr::remove_custom_model,
            commands::funasr::scan_hf_cache_models,
            commands::funasr::pick_gguf_file,
            commands::profile::set_audio_behavior,
            commands::profile::set_output_behavior,
            commands::profile::set_shortcuts_behavior,
            commands::profile::set_labs_config,
            commands::profile::set_keyboard_model_switching,
            commands::profile::set_global_transformations,
            commands::profile::get_active_keyboard_locale,
            commands::profile::get_scripting_status,
            commands::profile::scaffold_script_config,
            commands::profile::read_script_config,
            commands::profile::save_script_config,
            commands::profile::test_script_transformations,
            commands::profile::get_last_dictation_record,
            commands::profile::submit_output_fix,
            commands::profile::list_output_fixes,
            commands::meeting::get_meeting_status,
            commands::meeting::detect_meeting_call,
            commands::meeting::start_meeting_recording,
            commands::meeting::stop_meeting_recording,
            commands::meeting::list_meetings,
            commands::meeting::get_meeting,
            commands::meeting::delete_meeting,
            commands::meeting::export_meeting,
            commands::meeting::reprocess_meeting,
            commands::meeting::set_meetings_config,
            commands::support::get_diagnostics,
            commands::support::open_logs_folder,
            commands::support::get_free_disk_space,
        ]))
        .run(tauri::generate_context!())
        .expect("error while starting SpeakFlow");
}

fn mark_setup_once() -> bool {
    use std::sync::Once;

    static SETUP_ONCE: Once = Once::new();
    let mut first_run = false;
    SETUP_ONCE.call_once(|| {
        first_run = true;
    });
    first_run
}

fn spawn_funasr_startup(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let engine = utils::paths::read_engine_config();
        let state = app_handle.state::<AppState>();

        if utils::paths::is_online_engine(&engine) {
            let has_key = !state.read_online_asr_api_key().is_empty();
            state.set_funasr_ready(has_key);
            let label = match engine.as_str() {
                "alibaba-asr" => "Alibaba DashScope",
                _ => "GLM-ASR",
            };
            let _ = app_handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": if has_key { "ready" } else { "need_api_key" },
                    "message": if has_key {
                        format!("{} online service ready", label)
                    } else {
                        format!("Please configure the {} API Key", label)
                    },
                    "device": "cloud",
                    "gpu_name": serde_json::Value::Null,
                    "models_present": true,
                    "missing_models": [],
                }),
            );
            log::info!(
                "Online engine {}; skipping Python startup (has_key={})",
                engine,
                has_key
            );
            return;
        }

        log::info!("Starting the FunASR server in the background...");
        if let Err(err) = services::funasr_service::start_server(&app_handle, state.inner()).await {
            log::error!("Failed to start the FunASR server: {}", err);
            let _ = app_handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to start FunASR: {}", err),
                }),
            );
        }
    });
}

fn spawn_meeting_call_monitor(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_detected: Option<(String, String)> = None;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(4));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            if services::meeting_service::is_meeting_recording() {
                continue;
            }
            let cfg = app_handle
                .state::<AppState>()
                .with_profile(|p| p.meetings.clone());
            if !cfg.call_detection_enabled {
                last_detected = None;
                continue;
            }
            let Ok(hit) = tokio::task::spawn_blocking(move || {
                services::meeting_service::detect_active_call(&cfg)
            })
            .await
            else {
                continue;
            };
            if hit.active {
                let key = (
                    hit.app_name.clone().unwrap_or_default(),
                    hit.window_title.clone().unwrap_or_default(),
                );
                if last_detected.as_ref() != Some(&key) {
                    last_detected = Some(key);
                    let _ = app_handle.emit("meeting-call-detected", &hit);
                }
            } else {
                last_detected = None;
            }
        }
    });
}

fn spawn_openai_codex_oauth_prewarm(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        if let Err(err) =
            services::codex_oauth_service::prewarm_runtime_session(&app_handle, state.inner()).await
        {
            log::warn!("OpenAI Codex OAuth startup warm-up failed: {}", err);
        }
    });
}

fn spawn_grok_build_oauth_prewarm(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        if let Err(err) =
            services::grok_build_oauth_service::prewarm_runtime_session(&app_handle, state.inner())
                .await
        {
            log::warn!("Grok Build OAuth startup warm-up failed: {}", err);
        }
    });
}

fn spawn_profile_maintenance(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        const HOT_WORD_CLEANUP_INTERVAL_SECS: u64 = 24 * 60 * 60;
        log::info!(
            "Periodic hotword cleanup task started, interval {} seconds",
            HOT_WORD_CLEANUP_INTERVAL_SECS
        );

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            HOT_WORD_CLEANUP_INTERVAL_SECS,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            // Meeting audio expires on a clock, not only when another meeting ends.
            let (meetings, expire_audio) = app_handle.state::<AppState>().with_profile(|profile| {
                (profile.meetings.clone(), profile.labs.expire_meeting_audio)
            });
            let _ = tauri::async_runtime::spawn_blocking(move || {
                services::meeting_service::cleanup_expired_meeting_audio(&meetings, expire_audio);
            })
            .await;

            let should_save_profile = {
                let state = app_handle.state::<AppState>();
                let cleanup = state.update_profile_mut(services::profile_service::cleanup_profile);
                if cleanup.removed_hot_words > 0 || cleanup.removed_corrections > 0 {
                    log::info!(
                        "Periodic profile cleanup complete: removed {} hotwords, {} corrections",
                        cleanup.removed_hot_words,
                        cleanup.removed_corrections
                    );
                    true
                } else {
                    false
                }
            };

            if should_save_profile {
                let state = app_handle.state::<AppState>();
                if let Err(err) = services::profile_service::save_profile_async(state.inner()).await
                {
                    log::warn!(
                        "Failed to save the user profile after periodic hotword cleanup: {}",
                        err
                    );
                }
            }

            // LLM correction review: runs automatically when enabled and more than 24h since the last run
            let should_validate = {
                let state = app_handle.state::<AppState>();
                state.with_profile(|p| {
                    if !p.correction_validation_enabled {
                        return false;
                    }
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    now.saturating_sub(p.last_correction_validation)
                        >= HOT_WORD_CLEANUP_INTERVAL_SECS
                })
            };

            if should_validate {
                let state = app_handle.state::<AppState>();
                match commands::profile::run_correction_validation(&app_handle, state.inner()).await
                {
                    Ok(removed) if removed > 0 => {
                        log::info!(
                            "Periodic LLM correction review complete: deleted {}",
                            removed
                        );
                    }
                    Err(err) => {
                        log::warn!("Periodic LLM correction review failed: {}", err);
                    }
                    _ => {}
                }
            }
        }
    });
}

fn should_start_hidden(autostart_minimized: bool, mut args: impl Iterator<Item = String>) -> bool {
    autostart_minimized && args.any(|arg| arg == "--minimized")
}

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn stop_funasr_on_exit(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    services::audio_service::stop_microphone_level_monitor(state.inner());

    if let Some(recording) = state.recording.recording.lock().take() {
        match recording {
            RecordingSlot::Starting(s) => {
                s.stop_flag
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                s.stop_notify.notify_waiters();
            }
            RecordingSlot::Active(s) => {
                s.stop_flag
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                s.stop_notify.notify_waiters();
                if let Some(task) = s.interim_task {
                    task.abort();
                }
            }
        }
    }

    let funasr_process = state.engine.funasr_process.clone();

    tauri::async_runtime::block_on(async {
        match tokio::time::timeout(std::time::Duration::from_secs(2), funasr_process.lock()).await {
            Ok(mut guard) => {
                if let Some(ref mut process) = *guard {
                    log::info!("Stopping the FunASR process...");
                    let _ = process.child.start_kill();
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        process.child.wait(),
                    )
                    .await;
                }
            }
            Err(_) => {
                log::warn!("Timed out acquiring the FunASR process lock on exit; the process may not have been cleaned up");
            }
        }
    });
}

fn refresh_tray_icon(app: &tauri::AppHandle, icon_size: u32) {
    // Read canonical state so a delayed event from an older session cannot
    // clear the mark while a newer recording is active.
    let recording = app
        .state::<AppState>()
        .recording
        .snapshot()
        .is_some_and(|snapshot| snapshot.phase == state::RecordingPhase::Recording);
    let light = tray_icon::taskbar_is_light();
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Err(error) = tray.set_icon(Some(tray_icon::image(icon_size, recording, light))) {
            log::warn!("Failed to update the tray icon: {error}");
        }
    }
}

fn setup_system_tray(app_handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::Listener;

    let scale = app_handle
        .primary_monitor()
        .ok()
        .flatten()
        .map_or(1.0, |monitor| monitor.scale_factor());
    let icon_size = tray_icon::size_for_scale(scale);

    let show_item = MenuItemBuilder::with_id("show", "Show main window").build(app_handle)?;
    let hide_item = MenuItemBuilder::with_id("hide", "Hide main window").build(app_handle)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app_handle)?;
    let menu = MenuBuilder::new(app_handle)
        .item(&show_item)
        .item(&hide_item)
        .item(&quit_item)
        .build()?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon::image(
            icon_size,
            false,
            tray_icon::taskbar_is_light(),
        ))
        .tooltip("SpeakFlow - Voice to text")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => focus_main_window(app),
            "hide" => hide_main_window(app),
            "quit" => {
                log::info!("User requested to quit the app");
                stop_funasr_on_exit(app);
                app.exit(0);
            }
            _ => log::warn!("Unknown tray menu item: {:?}", event.id()),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                focus_main_window(tray.app_handle());
            }
        })
        .build(app_handle)?;

    let app = app_handle.clone();
    app_handle.listen("recording-state", move |event| {
        refresh_tray_icon(&app, icon_size);
        // The pill is the only on-screen feedback while dictating into another
        // app, so it follows the same event that drives the tray mark.
        let payload: serde_json::Value = serde_json::from_str(event.payload()).unwrap_or_default();
        let active = ["isStarting", "isRecording", "isProcessing"]
            .iter()
            .any(|key| {
                payload
                    .get(key)
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
            });
        commands::window::update_recording_pill(&app, active);
    });

    // The taskbar theme, and the accent it can be painted in, change while the
    // app sits idle, and Windows does not report either through Tauri's window
    // theme, so poll the settings and repaint when the palette would change.
    #[cfg(windows)]
    {
        let watcher = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let mut light = tray_icon::taskbar_is_light();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let next = tray_icon::taskbar_is_light();
                if next != light {
                    light = next;
                    refresh_tray_icon(&watcher, icon_size);
                }
            }
        });
    }

    Ok(())
}

#[cfg(test)]
mod startup_tests {
    use super::should_start_hidden;

    #[test]
    fn autostart_minimized_requires_both_the_preference_and_login_argument() {
        for (enabled, args, hidden) in [
            (false, vec!["speak-flow"], false),
            (true, vec!["speak-flow"], false),
            (false, vec!["speak-flow", "--minimized"], false),
            (true, vec!["speak-flow", "--minimized"], true),
            (true, vec!["speak-flow", "--minimized=false"], false),
        ] {
            assert_eq!(
                should_start_hidden(enabled, args.into_iter().map(str::to_owned)),
                hidden
            );
        }
    }
}
