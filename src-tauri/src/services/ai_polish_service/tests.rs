use super::{
    ai_polish_transport_plan, extract_edit_result, parse_structured_response,
    passthrough_unless_required, polish_hot_word_limit, push_user_terms,
    render_polish_user_content, structure_policy, PolishOutcome, BASE_SYSTEM_PROMPT,
    UNCONDITIONED_ENGINE_NOTE,
};
use crate::state::user_profile::{LlmReasoningMode, PolishStructureLevel};

#[test]
fn engines_without_recognition_vocabulary_send_all_terms_with_a_note() {
    assert_eq!(polish_hot_word_limit(true), 30);
    assert_eq!(polish_hot_word_limit(false), 100);

    let terms = vec!["Qwen".to_string(), "nginx".to_string()];
    let mut conditioned = String::new();
    push_user_terms(&mut conditioned, &terms, true);
    assert!(!conditioned.contains(UNCONDITIONED_ENGINE_NOTE));
    assert!(conditioned.contains("<term><![CDATA[Qwen]]></term>"));

    let mut unconditioned = String::new();
    push_user_terms(&mut unconditioned, &terms, false);
    assert!(unconditioned.starts_with(UNCONDITIONED_ENGINE_NOTE));
    assert!(unconditioned.contains("<term><![CDATA[nginx]]></term>"));

    let mut empty = String::new();
    push_user_terms(&mut empty, &[], false);
    assert!(empty.is_empty());
}

#[test]
fn polish_input_preserves_symbols_and_splits_cdata() {
    let content = render_polish_user_content(
        Some("<app_context><window_title><![CDATA[main.rs]]></window_title></app_context>"),
        "如果 a < b 并且原文里有 ]]> 就换行",
        true,
    );

    assert!(content
        .contains("<app_context><window_title><![CDATA[main.rs]]></window_title></app_context>"));
    assert!(content.contains(
        "<asr_text><![CDATA[如果 a < b 并且原文里有 ]]]]><![CDATA[> 就换行]]></asr_text>"
    ));
    assert!(content.contains(
            "<screen_context><![CDATA[A full-screen screenshot is attached as reference only when correcting the current ASR text.]]></screen_context>"
        ));
}

#[test]
fn explicit_repolish_never_reports_a_passthrough_as_success() {
    assert_eq!(
        passthrough_unless_required("原文", false, "缺少鉴权").expect("normal fallback"),
        "原文"
    );
    assert_eq!(
        passthrough_unless_required("原文", true, "缺少鉴权").unwrap_err(),
        "缺少鉴权"
    );
}

#[test]
fn passthrough_has_no_execution_metadata() {
    let outcome = PolishOutcome::passthrough("原文".into());

    assert_eq!(outcome.text, "原文");
    assert!(!outcome.executed);
    assert!(outcome.provider.is_none());
    assert!(outcome.model.is_none());
    assert!(outcome.screen_context_description.is_none());
}

#[test]
fn parse_structured_response_accepts_array_wrapper() {
    let parsed = parse_structured_response(
            r#"[{"polished":"字节现在AI优化的能力怎么样了呀？","corrections":[],"key_terms":["字节","AI"]}]"#,
        )
        .expect("should parse wrapped response");

    assert_eq!(parsed.polished, "字节现在AI优化的能力怎么样了呀？");
    assert!(parsed.corrections.is_empty());
    assert_eq!(parsed.key_terms, vec!["字节".to_string(), "AI".to_string()]);
}

#[test]
fn extract_edit_result_accepts_array_wrapper() {
    let result =
        extract_edit_result(r#"[{"result":"修改后的完整文本"}]"#).expect("should parse result");

    assert_eq!(result, "修改后的完整文本");
}

#[test]
fn parse_structured_response_accepts_cdata_wrapper() {
    let parsed = parse_structured_response(
        r#"<![CDATA[{"polished":"Yeah, baby.","corrections":[],"key_terms":[]}]]>"#,
    )
    .expect("should parse cdata wrapped response");

    assert_eq!(parsed.polished, "Yeah, baby.");
    assert!(parsed.corrections.is_empty());
    assert!(parsed.key_terms.is_empty());
}

#[test]
fn extract_edit_result_accepts_cdata_wrapper() {
    let result = extract_edit_result(r#"<![CDATA[{"result":"修改后的完整文本"}]]>"#)
        .expect("should parse cdata wrapped result");

    assert_eq!(result, "修改后的完整文本");
}

#[test]
fn parse_structured_response_accepts_xml_output_wrapper() {
    let parsed = parse_structured_response(
            r#"<output><![CDATA[{"polished":"Yeah, baby.","corrections":[],"key_terms":[]}]]></output>"#,
        )
        .expect("should parse xml wrapped response");

    assert_eq!(parsed.polished, "Yeah, baby.");
}

#[test]
fn parse_structured_response_extracts_json_from_explanatory_text() {
    let parsed = parse_structured_response(
            "好的，下面是结果：\n{\"polished\":\"Yeah, baby.\",\"corrections\":[],\"key_terms\":[]}\n请查收。",
        )
        .expect("should extract embedded json");

    assert_eq!(parsed.polished, "Yeah, baby.");
}

#[test]
fn extract_edit_result_accepts_xml_output_wrapper() {
    let result = extract_edit_result(r#"<output>{"result":"修改后的完整文本"}</output>"#)
        .expect("should parse xml wrapped result");

    assert_eq!(result, "修改后的完整文本");
}

#[test]
fn base_prompt_defines_self_repair_slot_precedence() {
    assert!(BASE_SYSTEM_PROMPT.contains("replaces the old value in the same intent slot"));
    assert!(BASE_SYSTEM_PROMPT.contains("target language"));
    assert!(BASE_SYSTEM_PROMPT.contains("recipient"));
}

#[test]
fn base_prompt_requires_phonetic_and_semantic_support() {
    assert!(BASE_SYSTEM_PROMPT.contains("phonetic or visual similarity"));
    assert!(BASE_SYSTEM_PROMPT.contains("replacement makes the meaning more plausible"));
    assert!(BASE_SYSTEM_PROMPT.contains("multiple interpretations are equally plausible"));
}

#[test]
fn base_prompt_keeps_few_shot_examples_small_and_diverse() {
    assert_eq!(BASE_SYSTEM_PROMPT.matches("<example>").count(), 6);
    assert!(BASE_SYSTEM_PROMPT.contains("Andriod and the Apple ecosystem"));
    assert!(BASE_SYSTEM_PROMPT.contains("Wednesday afternoon—no, Thursday"));
    assert!(BASE_SYSTEM_PROMPT.contains("Japanese—no, into English"));
    assert!(BASE_SYSTEM_PROMPT.contains("neither the program name nor the learned mapping"));
}

#[test]
fn base_prompt_stays_within_character_budget() {
    let character_count = BASE_SYSTEM_PROMPT.chars().count();
    assert!(
        character_count <= 4_000,
        "base prompt should stay concise, got {character_count} characters"
    );
}

#[test]
fn structure_levels_grant_progressively_broader_rewrite_permissions() {
    let off = structure_policy(PolishStructureLevel::Off);
    let light = structure_policy(PolishStructureLevel::Light);
    let balanced = structure_policy(PolishStructureLevel::Balanced);
    let strong = structure_policy(PolishStructureLevel::Strong);

    assert!(off.contains("Do not summarize, reorder, add headings, or create lists"));
    assert!(light.contains("explicitly dictated lists, steps, key points"));
    assert!(balanced.contains("limited reordering is allowed"));
    assert!(strong.contains("Reorder by topic and importance"));
    assert!(strong.contains("numbers, times, names, proper nouns, decisions"));
    assert!(strong.contains("Never add facts, conclusions, or action items"));
    assert!(strong.contains("Keep short, single-intent utterances concise and natural"));
}

#[test]
fn ai_polish_stream_stages_use_short_total_stream_budget() {
    let stages = ai_polish_transport_plan(LlmReasoningMode::ProviderDefault, 42, false);

    for stage in stages.iter().filter(|stage| stage.stream) {
        let timeout = stage
            .stream_total_timeout_secs
            .expect("AI polish streaming stages must carry a total timeout");
        assert!(
            (60..=120).contains(&timeout),
            "AI polish stream total timeout should be a short task budget, got {timeout}s"
        );
    }
}
