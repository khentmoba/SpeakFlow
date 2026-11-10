//! Stemmed and fuzzy BM25 search across dictation history and meeting transcripts.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::services::{history_service, meeting_service};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchDocument {
    pub id: String,
    pub kind: String, // "dictation" | "meeting"
    pub title: String,
    pub text: String,
    pub snippet: String,
    pub score: f64,
    pub created_at: i64,
    pub duration_sec: Option<f64>,
    pub app_name: Option<String>,
    pub mode: Option<String>,
    pub matched_terms: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IndexDocument {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub text: String,
    pub created_at: i64,
    pub duration_sec: Option<f64>,
    pub app_name: Option<String>,
    pub mode: Option<String>,
}

/// Tokenizes text into normalized ASCII stems + CJK unigrams/bigrams for BM25 indexing.
pub fn tokenize_terms(input: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut ascii_buf = String::new();
    let mut cjk_chars: Vec<char> = Vec::new();

    let flush_ascii = |buf: &mut String, out: &mut Vec<String>| {
        if !buf.is_empty() {
            let token = stem_english(buf);
            if !token.is_empty() {
                out.push(token);
            }
            buf.clear();
        }
    };

    let flush_cjk = |chars: &mut Vec<char>, out: &mut Vec<String>| {
        if chars.is_empty() {
            return;
        }
        for &ch in chars.iter() {
            out.push(ch.to_string());
        }
        for pair in chars.windows(2) {
            let mut bigram = String::with_capacity(6);
            bigram.push(pair[0]);
            bigram.push(pair[1]);
            out.push(bigram);
        }
        chars.clear();
    };

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk_chars, &mut terms);
            ascii_buf.push(ch.to_ascii_lowercase());
        } else if is_cjk(ch) {
            flush_ascii(&mut ascii_buf, &mut terms);
            cjk_chars.push(ch);
        } else {
            flush_ascii(&mut ascii_buf, &mut terms);
            flush_cjk(&mut cjk_chars, &mut terms);
        }
    }
    flush_ascii(&mut ascii_buf, &mut terms);
    flush_cjk(&mut cjk_chars, &mut terms);
    terms
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3040..=0x30FF | 0xAC00..=0xD7AF
    )
}

/// Lightweight English suffix stemmer (sufficient for dictation search without bringing an external NLP crate).
pub fn stem_english(word: &str) -> String {
    let w = word.to_ascii_lowercase();
    if w.len() <= 3 {
        return w;
    }
    for suffix in [
        "ational", "tional", "ization", "fulness", "ousness", "iveness",
    ] {
        if let Some(stem) = w.strip_suffix(suffix) {
            if stem.len() >= 3 {
                return stem.to_string();
            }
        }
    }
    for suffix in [
        "ments", "ment", "ness", "ings", "ing", "tion", "able", "ible", "ally",
    ] {
        if let Some(stem) = w.strip_suffix(suffix) {
            if stem.len() >= 3 {
                return stem.to_string();
            }
        }
    }
    for suffix in ["ies", "ied"] {
        if let Some(stem) = w.strip_suffix(suffix) {
            if stem.len() >= 2 {
                return format!("{stem}y");
            }
        }
    }
    for suffix in ["es", "ed", "ly"] {
        if let Some(stem) = w.strip_suffix(suffix) {
            if stem.len() >= 3 {
                return stem.to_string();
            }
        }
    }
    if let Some(stem) = w.strip_suffix('s') {
        if stem.len() >= 3 && !w.ends_with("ss") {
            return stem.to_string();
        }
    }
    w
}

/// Bounded Levenshtein distance <= 1 for fuzzy query expansion on tokens >= 4 chars.
fn within_edit_distance_one(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let alen = ab.len();
    let blen = bb.len();
    if alen.abs_diff(blen) > 1 {
        return false;
    }
    let mut i = 0usize;
    let mut j = 0usize;
    let mut edits = 0usize;
    while i < alen && j < blen {
        if ab[i] == bb[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        if alen == blen {
            i += 1;
            j += 1;
        } else if alen > blen {
            i += 1;
        } else {
            j += 1;
        }
    }
    if i < alen || j < blen {
        edits += 1;
    }
    edits <= 1
}

/// Scores documents using BM25 (`k1 = 1.5, b = 0.75`) plus fuzzy/prefix fallback.
pub fn rank_documents(docs: &[IndexDocument], query: &str, limit: usize) -> Vec<SearchDocument> {
    let trimmed_query = query.trim();
    if trimmed_query.is_empty() {
        let mut sorted: Vec<&IndexDocument> = docs.iter().collect();
        sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        return sorted
            .into_iter()
            .take(limit)
            .map(|doc| SearchDocument {
                id: doc.id.clone(),
                kind: doc.kind.clone(),
                title: doc.title.clone(),
                snippet: make_snippet(&doc.text, &[]),
                text: doc.text.clone(),
                score: 0.0,
                created_at: doc.created_at,
                duration_sec: doc.duration_sec,
                app_name: doc.app_name.clone(),
                mode: doc.mode.clone(),
                matched_terms: Vec::new(),
            })
            .collect();
    }

    let query_terms: Vec<String> = {
        let mut seen = HashSet::new();
        tokenize_terms(trimmed_query)
            .into_iter()
            .filter(|t| seen.insert(t.clone()))
            .collect()
    };
    if query_terms.is_empty() || docs.is_empty() {
        return Vec::new();
    }

    const K1: f64 = 1.5;
    const B: f64 = 0.75;

    let mut tokenized_docs: Vec<(Vec<String>, HashMap<String, usize>)> =
        Vec::with_capacity(docs.len());
    let mut doc_freq: HashMap<String, usize> = HashMap::new();
    let mut total_len = 0usize;

    for doc in docs {
        let combined = format!(
            "{} {} {}",
            doc.title,
            doc.app_name.as_deref().unwrap_or(""),
            doc.text
        );
        let tokens = tokenize_terms(&combined);
        total_len += tokens.len();
        let mut tf: HashMap<String, usize> = HashMap::new();
        for tok in &tokens {
            *tf.entry(tok.clone()).or_insert(0) += 1;
        }
        for term in tf.keys() {
            *doc_freq.entry(term.clone()).or_insert(0) += 1;
        }
        tokenized_docs.push((tokens, tf));
    }

    let num_docs = docs.len() as f64;
    let avg_dl = (total_len as f64 / num_docs).max(1.0);
    let lower_query = trimmed_query.to_ascii_lowercase();

    let mut scored = Vec::new();
    for (idx, doc) in docs.iter().enumerate() {
        let (tokens, tf_map) = &tokenized_docs[idx];
        let dl = (tokens.len() as f64).max(1.0);
        let mut score = 0.0;
        let mut matched_terms = Vec::new();

        for qterm in &query_terms {
            let exact_tf = tf_map.get(qterm).copied().unwrap_or(0) as f64;
            let (effective_tf, df_term) = if exact_tf > 0.0 {
                (exact_tf, qterm.as_str())
            } else if qterm.len() >= 4 && qterm.is_ascii() {
                // Fuzzy / prefix fallback at a slightly lower weight than exact stem hits.
                let mut best: Option<(&str, f64)> = None;
                for (doc_term, &count) in tf_map {
                    if !doc_term.is_ascii() {
                        continue;
                    }
                    if doc_term.starts_with(qterm.as_str()) {
                        let weighted = count as f64 * 0.75;
                        if best.is_none_or(|(_, s)| weighted > s) {
                            best = Some((doc_term.as_str(), weighted));
                        }
                    } else if within_edit_distance_one(qterm, doc_term) {
                        let weighted = count as f64 * 0.55;
                        if best.is_none_or(|(_, s)| weighted > s) {
                            best = Some((doc_term.as_str(), weighted));
                        }
                    }
                }
                match best {
                    Some((term, w)) => (w, term),
                    None => (0.0, qterm.as_str()),
                }
            } else {
                (0.0, qterm.as_str())
            };

            if effective_tf > 0.0 {
                let df = doc_freq.get(df_term).copied().unwrap_or(1) as f64;
                let idf = ((num_docs - df + 0.5) / (df + 0.5) + 1.0).ln().max(0.05);
                let norm =
                    effective_tf * (K1 + 1.0) / (effective_tf + K1 * (1.0 - B + B * (dl / avg_dl)));
                score += idf * norm;
                matched_terms.push(qterm.clone());
            }
        }

        if doc.text.to_ascii_lowercase().contains(&lower_query)
            || doc.title.to_ascii_lowercase().contains(&lower_query)
        {
            score += 1.5;
            if matched_terms.is_empty() {
                matched_terms.push(trimmed_query.to_string());
            }
        }

        if score > 0.0 {
            scored.push(SearchDocument {
                id: doc.id.clone(),
                kind: doc.kind.clone(),
                title: doc.title.clone(),
                snippet: make_snippet(&doc.text, &query_terms),
                text: doc.text.clone(),
                score,
                created_at: doc.created_at,
                duration_sec: doc.duration_sec,
                app_name: doc.app_name.clone(),
                mode: doc.mode.clone(),
                matched_terms,
            });
        }
    }

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    scored.truncate(limit);
    scored
}

fn make_snippet(text: &str, query_terms: &[String]) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 180 {
        return trimmed.to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    let hit_byte = query_terms
        .iter()
        .filter_map(|t| lower.find(&t.to_ascii_lowercase()))
        .min()
        .unwrap_or(0);
    let char_pos = trimmed[..hit_byte.min(trimmed.len())].chars().count();
    let start_char = char_pos.saturating_sub(45);
    let snippet: String = trimmed.chars().skip(start_char).take(160).collect();
    if start_char > 0 {
        format!("...{snippet}...")
    } else {
        format!("{snippet}...")
    }
}

/// Queries live SQLite dictation history and saved meeting transcripts with BM25 ranking.
pub async fn search_all(
    query: &str,
    kind: &str,
    limit: usize,
) -> Result<Vec<SearchDocument>, String> {
    let include_dictation = kind == "all" || kind == "dictation" || kind.is_empty();
    let include_meetings = kind == "all" || kind == "meeting";

    let mut docs = Vec::new();
    if include_dictation {
        if let Ok(records) = history_service::all_records().await {
            for r in records {
                if r.text.trim().is_empty() {
                    continue;
                }
                let title = r
                    .app_window_title
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("Dictation #{}", r.id));
                docs.push(IndexDocument {
                    id: format!("dictation:{}", r.id),
                    kind: "dictation".to_string(),
                    title,
                    text: r.text,
                    created_at: r.created_at,
                    duration_sec: r.duration_sec,
                    app_name: r.app_process,
                    mode: Some(r.mode),
                });
            }
        }
    }

    if include_meetings {
        for m in meeting_service::load_meetings() {
            let mut body = m.transcript_text.clone();
            if let Some(summary) = m.summary.as_deref() {
                if !summary.trim().is_empty() {
                    body = format!("{summary}\n\n{body}");
                }
            }
            if body.trim().is_empty() {
                continue;
            }
            docs.push(IndexDocument {
                id: format!("meeting:{}", m.id),
                kind: "meeting".to_string(),
                title: m.title,
                text: body,
                created_at: (m.started_at as i64) * 1000,
                duration_sec: Some(m.duration_sec),
                app_name: m.app_name,
                mode: Some("meeting".to_string()),
            });
        }
    }

    Ok(rank_documents(&docs, query, limit.clamp(1, 200)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_docs() -> Vec<IndexDocument> {
        vec![
            IndexDocument {
                id: "dictation:1".into(),
                kind: "dictation".into(),
                title: "Notes".into(),
                text: "Deploying Kubernetes ingress controllers with zero downtime".into(),
                created_at: 1000,
                duration_sec: Some(4.2),
                app_name: Some("Code.exe".into()),
                mode: Some("dictation".into()),
            },
            IndexDocument {
                id: "meeting:m1".into(),
                kind: "meeting".into(),
                title: "Architecture sync".into(),
                text: "We agreed to ship the WASAPI loopback capture pipeline and QuickJS sandbox"
                    .into(),
                created_at: 2000,
                duration_sec: Some(920.0),
                app_name: Some("Zoom".into()),
                mode: Some("meeting".into()),
            },
            IndexDocument {
                id: "dictation:2".into(),
                kind: "dictation".into(),
                title: "Email".into(),
                text: "Quick update about quarterly budgets and sprint planning".into(),
                created_at: 3000,
                duration_sec: Some(3.1),
                app_name: Some("OUTLOOK.EXE".into()),
                mode: Some("dictation".into()),
            },
        ]
    }

    #[test]
    fn stems_english_and_tokenizes_cjk_bigrams() {
        let tokens = tokenize_terms("Deploying controllers 会议纪要");
        assert!(tokens.contains(&"deploy".to_string()));
        assert!(tokens.contains(&"controller".to_string()));
        assert!(tokens.contains(&"会议".to_string()));
        assert!(tokens.contains(&"纪要".to_string()));
    }

    #[test]
    fn bm25_ranks_stemmed_and_fuzzy_matches_accurately() {
        let docs = sample_docs();
        let stemmed = rank_documents(&docs, "deployed controller", 10);
        assert_eq!(stemmed[0].id, "dictation:1");

        // Typo tolerance: "Kubernetis" -> matches "kubernete"/"kubernetes"
        let fuzzy = rank_documents(&docs, "Kubernetis", 10);
        assert_eq!(fuzzy[0].id, "dictation:1");

        let meeting = rank_documents(&docs, "loopback capture", 10);
        assert_eq!(meeting[0].id, "meeting:m1");
    }
}
