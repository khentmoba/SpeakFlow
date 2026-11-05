use super::*;

/// 剥离 LLM 返回的 markdown 代码块包裹（```json ... ``` 或 ``` ... ```）
pub(super) fn strip_markdown_code_block(s: &str) -> String {
    let trimmed = s.trim().trim_start_matches('\u{feff}');
    if trimmed.starts_with("```") {
        // 跳过第一行（```json 或 ```）
        let after_first_line = match trimmed.find('\n') {
            Some(pos) => &trimmed[pos + 1..],
            None => return trimmed.to_string(),
        };
        // 去掉末尾的 ```
        let content = after_first_line.trim_end();
        if let Some(stripped) = content.strip_suffix("```") {
            return stripped.trim().to_string();
        }
        return content.to_string();
    }
    trimmed.to_string()
}

pub(super) fn strip_cdata_wrapper(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(inner) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|value| value.strip_suffix("]]>"))
    {
        return inner.trim().to_string();
    }
    trimmed.to_string()
}

pub(super) fn strip_xml_wrapper(s: &str) -> String {
    let trimmed = s.trim();
    for tag in ["output", "response", "result"] {
        let prefix = format!("<{tag}>");
        let suffix = format!("</{tag}>");
        if let Some(inner) = trimmed
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix(&suffix))
        {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

pub(super) fn extract_json_segment(s: &str) -> Option<String> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    for (index, &(start_offset, start_char)) in chars.iter().enumerate() {
        let matching = match start_char {
            '{' => '}',
            '[' => ']',
            _ => continue,
        };

        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;

        for &(offset, ch) in &chars[index..] {
            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }
                match ch {
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    _ => {}
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                c if c == start_char => depth += 1,
                c if c == matching => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let end = offset + ch.len_utf8();
                        return Some(s[start_offset..end].trim().to_string());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

pub(super) fn normalize_structured_payload(raw: &str) -> String {
    let mut normalized = raw.trim().trim_start_matches('\u{feff}').to_string();
    loop {
        let next = strip_cdata_wrapper(&strip_xml_wrapper(&strip_markdown_code_block(&normalized)));
        if next == normalized {
            break;
        }
        normalized = next;
    }

    extract_json_segment(&normalized).unwrap_or(normalized)
}

pub(super) fn structured_response_from_value(
    value: serde_json::Value,
) -> Option<StructuredResponse> {
    match value {
        serde_json::Value::Object(_) => serde_json::from_value(value).ok(),
        serde_json::Value::Array(items) => items
            .into_iter()
            .find_map(|item| serde_json::from_value::<StructuredResponse>(item).ok()),
        _ => None,
    }
}

pub(super) fn parse_structured_response(raw: &str) -> Option<StructuredResponse> {
    let json_content = normalize_structured_payload(raw);
    let value = serde_json::from_str::<serde_json::Value>(&json_content).ok()?;
    structured_response_from_value(value)
}

pub(super) fn extract_edit_result(raw: &str) -> Option<String> {
    let json_content = normalize_structured_payload(raw);
    let value = serde_json::from_str::<serde_json::Value>(&json_content).ok()?;

    match value {
        serde_json::Value::Object(map) => map
            .get("result")
            .and_then(|value| value.as_str())
            .map(String::from),
        serde_json::Value::Array(items) => items.into_iter().find_map(|item| {
            item.get("result")
                .and_then(|value| value.as_str())
                .map(String::from)
        }),
        _ => None,
    }
}
