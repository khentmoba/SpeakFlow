pub mod ai_polish_service;
pub mod alibaba_asr_service;
pub mod assistant_service;
pub mod audio_service;
pub mod bing_search_service;
pub mod browser_domain;
pub mod claude_oauth_service;
pub mod cli_service;
pub mod codex_oauth_service;
pub mod context_awareness;
pub mod custom_models;
pub mod dictation_tools;
pub mod download_service;
pub mod external_correction_service;
pub mod file_transcription;
pub mod filler_words;
pub mod funasr_service;
pub mod glm_asr_service;
pub mod grok_build_oauth_service;
pub mod history_export;
pub mod history_service;
pub mod http_api_service;
pub mod jev_review;
pub mod jev_service;
pub mod jev_tasks;
pub mod keyboard_layout;
pub mod llm_client;
pub mod llm_provider;
pub mod local_only;
pub mod magpie_service;
pub mod meeting_service;
mod oauth_callback;
pub mod profile_service;
pub mod qwen_hotword_service;
pub mod screen_capture_service;
pub mod screen_vision_service;
pub mod scripting_service;
pub mod search_service;
pub mod selection_service;
pub mod snippets;
pub mod spoken_formatting;
pub mod text_transform;
pub mod tone_modes;
pub mod transcript_cleanup;
pub mod voice_commands;
pub mod web_search_service;

#[cfg(test)]
mod ai_polish_transport_retry_tests;
#[cfg(test)]
mod claude_oauth_tests;
#[cfg(test)]
mod codex_oauth_prewarm_tests;
#[cfg(test)]
mod deepseek_native_web_search_tests;
#[cfg(test)]
mod grok_build_oauth_tests;
#[cfg(test)]
mod jev_service_tests;
#[cfg(test)]
mod jev_tasks_tests;
#[cfg(test)]
mod llm_endpoint_resolution_tests;
#[cfg(test)]
mod llm_reasoning_negotiation_tests;
#[cfg(test)]
mod openai_fast_mode_oauth_tests;
#[cfg(test)]
mod openrouter_regression_tests;
#[cfg(test)]
mod perf_benchmarks;
