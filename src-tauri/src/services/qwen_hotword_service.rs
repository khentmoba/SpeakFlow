use pinyin::ToPinyin;
use std::collections::HashSet;

use crate::state::user_profile::{
    CorrectionPattern, CorrectionSource, HotWord, HotWordSource, UserProfile,
};

const MAX_ASR_HOT_WORDS: usize = 100;
const MAX_ASR_ALIASES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotWordCorrection {
    pub text: String,
    pub replacements: usize,
}

#[derive(Debug, Clone)]
struct ReplacementCandidate<'a> {
    start: usize,
    end: usize,
    replacement: &'a str,
    score: u32,
    rank: usize,
}

#[derive(Debug, Clone, Copy)]
struct WordSpan {
    start: usize,
    end: usize,
}

pub fn apply_profile_hotword_corrections(state: &crate::state::AppState, text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let started = std::time::Instant::now();
    let correction = state.with_profile(|profile| correct_qwen_profile_terms(text, profile));
    if correction.replacements > 0 {
        log::info!(
            "Local hotword correction applied: {} replacements in {} us",
            correction.replacements,
            started.elapsed().as_micros()
        );
    }
    correction.text
}

pub fn correct_qwen_profile_terms(text: &str, profile: &UserProfile) -> HotWordCorrection {
    let hot_words = correct_qwen_hot_words(text, &profile.hot_words);
    let aliases = correct_known_aliases(
        &hot_words.text,
        &profile.hot_words,
        &profile.correction_patterns,
    );

    HotWordCorrection {
        text: aliases.text,
        replacements: aliases.replacements + hot_words.replacements,
    }
}

pub fn correct_qwen_hot_words(text: &str, hot_words: &[HotWord]) -> HotWordCorrection {
    if text.is_empty() || hot_words.is_empty() {
        return HotWordCorrection {
            text: text.to_owned(),
            replacements: 0,
        };
    }

    let ranked = ranked_hot_words(hot_words);

    let chars = indexed_chars(text);
    let ascii_words = ascii_word_spans(text);
    let mut candidates = Vec::new();

    for (rank, hot_word) in ranked.into_iter().enumerate() {
        let hot_text = hot_word.text.trim();
        if hot_text.is_empty() {
            continue;
        }

        if requires_literal_matching(hot_text) {
            collect_literal_candidates(text, hot_word, hot_text, rank, &mut candidates);
        } else if hot_text.chars().all(is_han) {
            collect_han_candidates(text, &chars, hot_word, hot_text, rank, &mut candidates);
        } else if hot_text.is_ascii() && hot_text.chars().any(|ch| ch.is_ascii_alphanumeric()) {
            collect_ascii_candidates(
                text,
                &ascii_words,
                hot_word,
                hot_text,
                rank,
                &mut candidates,
            );
        }
    }

    apply_candidates(text, candidates)
}

fn ranked_hot_words(hot_words: &[HotWord]) -> Vec<&HotWord> {
    let mut ranked: Vec<_> = hot_words.iter().collect();
    ranked.sort_by(|a, b| b.weight.cmp(&a.weight).then(b.use_count.cmp(&a.use_count)));
    ranked
        .into_iter()
        .enumerate()
        .filter(|(rank, word)| *rank < MAX_ASR_HOT_WORDS || word.source == HotWordSource::User)
        .map(|(_, word)| word)
        .collect()
}

fn requires_literal_matching(term: &str) -> bool {
    (!term.is_ascii() && !term.chars().all(is_han))
        || !term.starts_with(|ch: char| ch.is_ascii_alphanumeric() || is_han(ch))
        || !term.ends_with(|ch: char| ch.is_ascii_alphanumeric() || is_han(ch))
}

fn collect_literal_candidates<'a>(
    text: &str,
    hot_word: &'a HotWord,
    hot_text: &'a str,
    rank: usize,
    candidates: &mut Vec<ReplacementCandidate<'a>>,
) {
    if hot_word.source != HotWordSource::User && !has_canonical_ascii_style(hot_text) {
        return;
    }
    let term_len = hot_text.chars().count();
    for (start, _) in text.char_indices() {
        let mut chars = text[start..].char_indices();
        let Some((last, ch)) = chars.nth(term_len - 1) else {
            break;
        };
        let end = start + last + ch.len_utf8();
        let raw = &text[start..end];
        if raw == hot_text
            || !raw
                .chars()
                .flat_map(char::to_lowercase)
                .eq(hot_text.chars().flat_map(char::to_lowercase))
            || !has_ascii_boundaries(text, start, end)
        {
            continue;
        }
        candidates.push(ReplacementCandidate {
            start,
            end,
            replacement: hot_text,
            score: 1_000 + term_len as u32,
            rank,
        });
    }
}

fn correct_known_aliases(
    text: &str,
    hot_words: &[HotWord],
    correction_patterns: &[CorrectionPattern],
) -> HotWordCorrection {
    if text.is_empty()
        || !correction_patterns.iter().any(|pattern| {
            (pattern.manual || !hot_words.is_empty()) && alias_may_match(text, pattern)
        })
    {
        return HotWordCorrection {
            text: text.to_owned(),
            replacements: 0,
        };
    }

    let hot_targets: HashSet<String> = ranked_hot_words(hot_words)
        .into_iter()
        .map(|hot_word| normalize_profile_term(hot_word.text.trim()))
        .filter(|normalized| !normalized.is_empty())
        .collect();

    let mut aliases: Vec<&CorrectionPattern> = correction_patterns
        .iter()
        .filter(|pattern| is_safe_alias(pattern, &hot_targets))
        .collect();
    aliases.sort_by(|a, b| {
        b.manual
            .cmp(&a.manual)
            .then(b.count.cmp(&a.count))
            .then(b.last_seen.cmp(&a.last_seen))
    });
    aliases.truncate(MAX_ASR_ALIASES);

    let mut candidates = Vec::new();
    for (rank, alias) in aliases.into_iter().enumerate() {
        collect_alias_candidates(text, alias, rank, &mut candidates);
    }

    apply_candidates(text, candidates)
}

fn is_safe_alias(pattern: &CorrectionPattern, hot_targets: &HashSet<String>) -> bool {
    let original = pattern.original.trim();
    let corrected = pattern.corrected.trim();
    if original.is_empty() || corrected.is_empty() || original == corrected {
        return false;
    }

    let original_normalized = normalize_profile_term(original);
    let corrected_normalized = normalize_profile_term(corrected);
    if pattern.manual {
        // The user typed this rule, so it needs no vocabulary target and may
        // map any spelling. Very short originals would still hit too often.
        let min_length = if original.is_ascii() { 2 } else { 1 };
        return original_normalized.chars().count() >= min_length
            && original.chars().count() <= 80
            && corrected.chars().count() <= 80;
    }
    if !hot_targets.contains(&corrected_normalized) {
        return false;
    }

    let original_is_ascii = original.is_ascii();
    let corrected_is_ascii = corrected.is_ascii();
    let original_is_han = original.chars().all(is_han);
    let corrected_is_han = corrected.chars().all(is_han);
    let same_script =
        (original_is_ascii && corrected_is_ascii) || (original_is_han && corrected_is_han);
    if !same_script {
        return false;
    }

    let min_length = if original_is_ascii { 3 } else { 2 };
    if original_normalized.chars().count() < min_length
        || corrected_normalized.chars().count() < min_length
        || original.chars().count() > 80
        || corrected.chars().count() > 80
    {
        return false;
    }

    match pattern.source {
        CorrectionSource::Ai => true,
        CorrectionSource::User => {
            if !original_is_ascii {
                return false;
            }
            let original_words = ascii_word_spans(original).len();
            original_words > 1 || levenshtein(&original_normalized, &corrected_normalized) <= 1
        }
    }
}

fn collect_alias_candidates<'a>(
    text: &str,
    alias: &'a CorrectionPattern,
    rank: usize,
    candidates: &mut Vec<ReplacementCandidate<'a>>,
) {
    let original = alias.original.trim();
    let corrected = alias.corrected.trim();
    // Manual ASCII rules ignore case. ASCII lowercasing keeps byte offsets, so
    // matches in the lowered copy index the original text.
    let lowered;
    let (haystack, needle) = if alias.manual && original.is_ascii() {
        lowered = (text.to_ascii_lowercase(), original.to_ascii_lowercase());
        (lowered.0.as_str(), lowered.1.as_str())
    } else {
        (text, original)
    };
    for (start, matched) in haystack.match_indices(needle) {
        let end = start + matched.len();
        if &text[start..end] == corrected {
            continue;
        }
        if original.is_ascii() && !has_ascii_boundaries(text, start, end) {
            continue;
        }
        candidates.push(ReplacementCandidate {
            start,
            end,
            replacement: corrected,
            score: 2_000 + alias.count.min(1_000) + original.chars().count() as u32,
            rank,
        });
    }
}

fn alias_may_match(text: &str, pattern: &CorrectionPattern) -> bool {
    let original = pattern.original.trim();
    if original.is_empty() {
        return false;
    }
    if pattern.manual && original.is_ascii() {
        return text
            .to_ascii_lowercase()
            .contains(&original.to_ascii_lowercase());
    }
    text.contains(original)
}

fn apply_candidates<'a>(
    text: &str,
    mut candidates: Vec<ReplacementCandidate<'a>>,
) -> HotWordCorrection {
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.rank.cmp(&b.rank))
            .then(a.start.cmp(&b.start))
    });

    let mut selected: Vec<ReplacementCandidate<'_>> = Vec::new();
    for candidate in candidates {
        let overlaps = selected
            .iter()
            .any(|kept| candidate.start < kept.end && kept.start < candidate.end);
        if !overlaps {
            selected.push(candidate);
        }
    }

    selected.sort_by(|a, b| b.start.cmp(&a.start));
    let replacements = selected.len();
    let mut corrected = text.to_owned();
    for candidate in selected {
        corrected.replace_range(candidate.start..candidate.end, candidate.replacement);
    }

    HotWordCorrection {
        text: corrected,
        replacements,
    }
}

fn collect_han_candidates<'a>(
    text: &str,
    chars: &[(usize, char)],
    hot_word: &'a HotWord,
    hot_text: &'a str,
    rank: usize,
    candidates: &mut Vec<ReplacementCandidate<'a>>,
) {
    let hot_chars: Vec<char> = hot_text.chars().collect();
    let hot_len = hot_chars.len();
    let is_manual = hot_word.source == HotWordSource::User && hot_word.weight >= 3;
    let min_len = if is_manual { 2 } else { 3 };
    if hot_len < min_len || hot_len > chars.len() {
        return;
    }

    let Some(hot_pinyin) = pinyin_signature(&hot_chars) else {
        return;
    };

    let min_shared = if is_manual {
        1.max(hot_len / 3)
    } else {
        1.max(hot_len.div_ceil(2))
    };

    for start_index in 0..=chars.len() - hot_len {
        let window = &chars[start_index..start_index + hot_len];
        if !window.iter().all(|(_, ch)| is_han(*ch)) {
            continue;
        }

        if window
            .iter()
            .map(|(_, ch)| *ch)
            .eq(hot_chars.iter().copied())
        {
            continue;
        }

        let shared_chars = window
            .iter()
            .zip(&hot_chars)
            .filter(|((_, ch), hot)| ch == *hot)
            .count();
        if shared_chars < min_shared {
            continue;
        }

        let pinyin_matches = window.iter().zip(&hot_pinyin).all(|((_, ch), hot_pi)| {
            ch.to_pinyin()
                .map(|p| p.plain() == *hot_pi)
                .unwrap_or(false)
        });
        if !pinyin_matches {
            continue;
        }

        let start = window[0].0;
        let end = chars
            .get(start_index + hot_len)
            .map(|(byte_index, _)| *byte_index)
            .unwrap_or(text.len());
        candidates.push(ReplacementCandidate {
            start,
            end,
            replacement: hot_text,
            score: 900 + (shared_chars as u32 * 20) + (hot_len as u32 * 5),
            rank,
        });
    }
}

fn collect_ascii_candidates<'a>(
    text: &str,
    words: &[WordSpan],
    hot_word: &'a HotWord,
    hot_text: &'a str,
    rank: usize,
    candidates: &mut Vec<ReplacementCandidate<'a>>,
) {
    let is_manual = hot_word.source == HotWordSource::User && hot_word.weight >= 3;
    if !is_manual && !has_canonical_ascii_style(hot_text) {
        return;
    }

    let hot_normalized = normalize_ascii(hot_text);
    if hot_normalized.is_empty() {
        return;
    }

    let hot_sound = is_manual
        .then(|| sound_key(&hot_normalized))
        .filter(|key| key.len() >= MIN_SOUND_KEY_LEN);
    let max_sound_distance = (hot_normalized.len() / 2).clamp(1, 3);

    let hot_word_count = ascii_word_spans(hot_text).len().max(1);
    let min_words = hot_word_count.saturating_sub(1).max(1);
    let max_words = hot_word_count + 1;
    for start_index in 0..words.len() {
        for word_count in min_words..=max_words {
            let Some(end_index) = start_index.checked_add(word_count - 1) else {
                continue;
            };
            let Some(last_word) = words.get(end_index) else {
                continue;
            };
            let start = words[start_index].start;
            let end = last_word.end;
            let raw = &text[start..end];
            if !raw.is_ascii() || raw == hot_text || !has_ascii_boundaries(text, start, end) {
                continue;
            }

            let candidate_normalized = normalize_ascii(raw);
            if candidate_normalized.is_empty() {
                continue;
            }

            if candidate_normalized == hot_normalized {
                candidates.push(ReplacementCandidate {
                    start,
                    end,
                    replacement: hot_text,
                    score: 1000 + hot_normalized.len() as u32,
                    rank,
                });
                continue;
            }

            if !is_manual || is_simple_inflection(&candidate_normalized, &hot_normalized) {
                continue;
            }

            let length_delta = candidate_normalized.len().abs_diff(hot_normalized.len());
            let max_distance = if hot_normalized.len() >= 10 { 2 } else { 1 };
            if hot_normalized.len() >= 5 && length_delta <= max_distance {
                let distance = levenshtein(&candidate_normalized, &hot_normalized);
                if distance <= max_distance {
                    candidates.push(ReplacementCandidate {
                        start,
                        end,
                        replacement: hot_text,
                        score: 800 + hot_normalized.len() as u32 - (distance as u32 * 50),
                        rank,
                    });
                    continue;
                }
            }

            // Engines without recognition vocabulary often hear a name as a
            // similar-sounding spelling. Accept those when the sounds match,
            // the spelling is still close, and the words are not everyday English.
            let Some(hot_sound) = hot_sound.as_deref() else {
                continue;
            };
            if length_delta > max_sound_distance
                || is_common_english_phrase(raw)
                || sound_key(&candidate_normalized) != hot_sound
            {
                continue;
            }
            let distance = levenshtein(&candidate_normalized, &hot_normalized);
            if distance > max_sound_distance {
                continue;
            }
            candidates.push(ReplacementCandidate {
                start,
                end,
                replacement: hot_text,
                score: 700 + hot_normalized.len() as u32 - (distance as u32 * 50),
                rank,
            });
        }
    }
}

const MIN_SOUND_KEY_LEN: usize = 3;

/// A simplified Metaphone key for lowercase ASCII: vowels drop out and
/// consonant spellings that sound alike share one symbol.
fn sound_key(word: &str) -> String {
    let bytes = word.as_bytes();
    let at = |index: usize| bytes.get(index).copied().unwrap_or(0);
    let is_vowel = |byte: u8| matches!(byte, b'a' | b'e' | b'i' | b'o' | b'u' | b'y');
    let is_front_vowel = |byte: u8| matches!(byte, b'e' | b'i' | b'y');

    let mut key = String::with_capacity(bytes.len());
    let push = |key: &mut String, sound: &str| {
        for ch in sound.chars() {
            if !key.ends_with(ch) {
                key.push(ch);
            }
        }
    };
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let next = at(index + 1);
        let mut skip = 1;
        match byte {
            _ if is_vowel(byte) => {}
            b'0'..=b'9' => push(&mut key, &(byte as char).to_string()),
            b'c' if next == b'h' => {
                push(&mut key, "x");
                skip = 2;
            }
            b'c' if is_front_vowel(next) => push(&mut key, "s"),
            b'c' => push(&mut key, "k"),
            b'd' if next == b'g' && is_front_vowel(at(index + 2)) => {
                push(&mut key, "j");
                skip = 2;
            }
            b'g' if next == b'h' => skip = 2,
            b'g' if next == b'n' && (index == 0 || index + 2 == bytes.len()) => {}
            b'g' if is_front_vowel(next) => push(&mut key, "j"),
            b'g' => push(&mut key, "k"),
            b'h' if index == 0 && is_vowel(next) => push(&mut key, "h"),
            b'h' => {}
            b'k' if index == 0 && next == b'n' => {}
            b'p' if next == b'h' => {
                push(&mut key, "f");
                skip = 2;
            }
            b'q' if next == b'u' => {
                push(&mut key, "kw");
                skip = 2;
            }
            b'q' => push(&mut key, "k"),
            b's' if next == b'h' => {
                push(&mut key, "x");
                skip = 2;
            }
            b't' if next == b'h' => {
                push(&mut key, "0");
                skip = 2;
            }
            b'w' if index == 0 && next == b'r' => {}
            b'w' if next == b'h' => {
                push(&mut key, "w");
                skip = 2;
            }
            b'w' if is_vowel(next) => push(&mut key, "w"),
            b'w' => {}
            b'x' if index == 0 => push(&mut key, "s"),
            b'x' => push(&mut key, "ks"),
            b'z' => push(&mut key, "s"),
            _ => push(&mut key, &(byte as char).to_string()),
        }
        index += skip;
    }
    key
}

/// True when every word in the span is everyday English. Sound matching never
/// rewrites those, so adding a name like "Nuxt" cannot replace "next".
fn is_common_english_phrase(text: &str) -> bool {
    ascii_word_spans(text).iter().all(|span| {
        let word = text[span.start..span.end].to_ascii_lowercase();
        COMMON_ENGLISH_WORDS.binary_search(&word.as_str()).is_ok()
    })
}

/// Frequent English words, sorted for binary search.
#[rustfmt::skip]
const COMMON_ENGLISH_WORDS: &[&str] = &[
    "about", "above", "across", "act", "add", "after", "again", "against", "age", "ago", "air",
    "all", "almost", "alone", "along", "already", "also", "always", "among", "and", "another",
    "answer", "any", "anyone", "anything", "app", "area", "arm", "around", "art", "ask", "away",
    "back", "bad", "bag", "bank", "base", "be", "bear", "beat", "became", "because", "become",
    "bed", "been", "before", "began", "begin", "behind", "being", "believe", "below", "best",
    "better", "between", "big", "bill", "bit", "black", "blue", "board", "body", "book", "born",
    "both", "box", "boy", "break", "bring", "brought", "build", "built", "business", "but",
    "buy", "call", "came", "can", "cannot", "car", "card", "care", "carry", "case", "cat",
    "catch", "cause", "cent", "center", "certain", "chance", "change", "check", "child",
    "choose", "city", "class", "clean", "clear", "close", "cloud", "code", "cold", "come",
    "common", "company", "copy", "cost", "could", "count", "country", "course", "cover",
    "create", "cut", "dark", "data", "date", "day", "dead", "deal", "dear", "decide", "deep",
    "did", "die", "different", "do", "does", "dog", "done", "door", "down", "draw", "drive",
    "during", "each", "early", "east", "easy", "eat", "edge", "else", "end", "engine", "enough",
    "even", "evening", "event", "ever", "every", "exact", "example", "eye", "face", "fact",
    "fall", "family", "far", "fast", "father", "feel", "few", "field", "file", "fill", "final",
    "find", "fine", "fire", "first", "fix", "floor", "flow", "fly", "follow", "food", "foot",
    "for", "force", "form", "found", "free", "friend", "from", "front", "full", "game", "gave",
    "get", "girl", "give", "given", "go", "going", "gone", "good", "got", "great", "green",
    "ground", "group", "grow", "had", "half", "hand", "happen", "hard", "has", "have", "he",
    "head", "hear", "heard", "heart", "held", "help", "her", "here", "high", "him", "his",
    "hit", "hold", "home", "hope", "horse", "hot", "hour", "house", "how", "however", "hub",
    "idea", "if", "important", "in", "into", "is", "issue", "it", "its", "job", "just", "keep",
    "kept", "key", "kind", "king", "knew", "know", "known", "land", "large", "last", "late",
    "later", "laugh", "lead", "learn", "least", "leave", "left", "less", "let", "letter",
    "life", "light", "like", "line", "list", "listen", "little", "live", "load", "long", "look",
    "lose", "lost", "lot", "love", "low", "made", "main", "make", "man", "many", "mark", "may",
    "maybe", "me", "mean", "meet", "men", "might", "mind", "minute", "miss", "model", "moment",
    "money", "month", "more", "morning", "most", "mother", "move", "much", "music", "must",
    "my", "name", "near", "need", "never", "new", "next", "nice", "night", "no", "none", "not",
    "note", "nothing", "notice", "now", "number", "of", "off", "office", "often", "old", "on",
    "once", "one", "only", "open", "or", "order", "other", "our", "out", "over", "own", "page",
    "paper", "part", "party", "pass", "past", "pay", "people", "person", "pick", "picture",
    "piece", "place", "plan", "play", "point", "power", "press", "pretty", "print", "problem",
    "program", "put", "question", "quick", "quite", "rain", "ran", "rather", "reach", "read",
    "ready", "real", "reason", "record", "red", "remember", "rest", "result", "right", "ring",
    "river", "road", "rock", "room", "round", "rule", "run", "said", "same", "save", "saw",
    "say", "school", "sea", "second", "see", "seem", "seen", "send", "sense", "sent", "set",
    "several", "shall", "she", "ship", "short", "should", "show", "side", "simple", "since",
    "sing", "sit", "size", "small", "so", "some", "something", "song", "soon", "sorry", "sound",
    "south", "speak", "special", "stand", "start", "state", "stay", "step", "still", "stop",
    "story", "street", "strong", "study", "such", "sun", "sure", "system", "table", "take",
    "talk", "team", "tell", "ten", "term", "test", "text", "than", "thank", "that", "the",
    "their", "them", "then", "there", "these", "they", "thing", "think", "this", "those",
    "though", "thought", "three", "through", "time", "to", "today", "together", "told", "too",
    "took", "top", "toward", "town", "tree", "true", "try", "turn", "two", "type", "under",
    "until", "up", "upon", "us", "use", "used", "user", "usual", "very", "view", "voice",
    "wait", "walk", "wall", "want", "war", "warm", "was", "watch", "water", "way", "we", "week",
    "well", "went", "were", "west", "what", "when", "where", "which", "while", "white", "who",
    "whole", "why", "will", "wind", "window", "with", "within", "without", "woman", "word",
    "work", "world", "would", "write", "wrong", "year", "yes", "yet", "you", "young", "your",
];

fn indexed_chars(text: &str) -> Vec<(usize, char)> {
    text.char_indices().collect()
}

fn ascii_word_spans(text: &str) -> Vec<WordSpan> {
    let mut spans = Vec::new();
    let mut start = None;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_alphanumeric() {
            start.get_or_insert(index);
        } else if let Some(word_start) = start.take() {
            spans.push(WordSpan {
                start: word_start,
                end: index,
            });
        }
    }
    if let Some(word_start) = start {
        spans.push(WordSpan {
            start: word_start,
            end: text.len(),
        });
    }
    spans
}

fn normalize_ascii(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn normalize_profile_term(text: &str) -> String {
    text.chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if is_han(ch) {
                Some(ch)
            } else {
                None
            }
        })
        .collect()
}

fn has_ascii_boundaries(text: &str, start: usize, end: usize) -> bool {
    let before_is_word = text[..start]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_alphanumeric() && !is_han(ch));
    let after_is_word = text[end..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphanumeric() && !is_han(ch));
    !before_is_word && !after_is_word
}

fn has_canonical_ascii_style(text: &str) -> bool {
    text.chars().filter(|ch| ch.is_ascii_uppercase()).count() >= 2
}

fn is_simple_inflection(candidate: &str, hot_word: &str) -> bool {
    const SUFFIXES: &[&str] = &["s", "es", "ed", "ing"];
    SUFFIXES.iter().any(|suffix| {
        candidate
            .strip_prefix(hot_word)
            .is_some_and(|remainder| remainder == *suffix)
            || hot_word
                .strip_prefix(candidate)
                .is_some_and(|remainder| remainder == *suffix)
    })
}

fn pinyin_signature(chars: &[char]) -> Option<Vec<&'static str>> {
    chars
        .iter()
        .map(|ch| ch.to_pinyin().map(|pinyin| pinyin.plain()))
        .collect()
}

fn is_han(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2a6df}'
            | '\u{2a700}'..='\u{2b73f}'
            | '\u{2b740}'..='\u{2b81f}'
            | '\u{2b820}'..='\u{2ceaf}'
            | '\u{2ceb0}'..='\u{2ebef}'
            | '\u{30000}'..='\u{3134f}'
    )
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right = right.as_bytes();
    let r_len = right.len();
    if r_len <= 64 {
        let mut previous = [0usize; 65];
        let mut current = [0usize; 65];
        for (i, p) in previous.iter_mut().enumerate().take(r_len + 1) {
            *p = i;
        }

        for (left_index, &left_byte) in left.as_bytes().iter().enumerate() {
            current[0] = left_index + 1;
            for (right_index, &right_byte) in right.iter().enumerate() {
                let substitution = previous[right_index] + usize::from(left_byte != right_byte);
                current[right_index + 1] = (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(substitution);
            }
            previous[..=r_len].copy_from_slice(&current[..=r_len]);
        }
        previous[r_len]
    } else {
        let mut previous: Vec<usize> = (0..=r_len).collect();
        let mut current = vec![0; r_len + 1];

        for (left_index, &left_byte) in left.as_bytes().iter().enumerate() {
            current[0] = left_index + 1;
            for (right_index, &right_byte) in right.iter().enumerate() {
                let substitution = previous[right_index] + usize::from(left_byte != right_byte);
                current[right_index + 1] = (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(substitution);
            }
            std::mem::swap(&mut previous, &mut current);
        }
        previous[r_len]
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn correction(
        original: &str,
        corrected: &str,
        count: u32,
        source: CorrectionSource,
    ) -> CorrectionPattern {
        CorrectionPattern {
            original: original.to_owned(),
            corrected: corrected.to_owned(),
            count,
            last_seen: 0,
            source,
            manual: false,
        }
    }

    fn hot_word(text: &str, source: HotWordSource, weight: u8, use_count: u32) -> HotWord {
        HotWord {
            text: text.to_owned(),
            weight,
            source,
            use_count,
            last_used: 0,
        }
    }

    fn user(text: &str) -> HotWord {
        hot_word(text, HotWordSource::User, 3, 0)
    }

    fn learned(text: &str) -> HotWord {
        hot_word(text, HotWordSource::Learned, 2, 3)
    }

    fn representative_hot_words() -> Vec<HotWord> {
        vec![
            user("github"),
            user("claude code"),
            user("Oura Ring"),
            user("openclaw"),
            user("typeless"),
            user("agent"),
            user("佳明"),
            user("轻语"),
            learned("同济大学"),
            learned("划词助手"),
            learned("第一性原理"),
            learned("VS Code"),
            learned("Cloud"),
            learned("欧盟"),
        ]
    }

    fn maximum_hotword_load() -> Vec<HotWord> {
        let mut hot_words = representative_hot_words();
        while hot_words.len() < MAX_ASR_HOT_WORDS {
            hot_words.push(learned(&format!("backgroundterm{:03}", hot_words.len())));
        }
        hot_words
    }

    fn maximum_profile_load() -> UserProfile {
        let hot_words = maximum_hotword_load();
        let correction_patterns = hot_words
            .iter()
            .filter(|hot_word| hot_word.text.starts_with("backgroundterm"))
            .enumerate()
            .map(|(index, hot_word)| {
                correction(
                    &format!("misheardterm{index:03}"),
                    &hot_word.text,
                    100 - index.min(99) as u32,
                    CorrectionSource::Ai,
                )
            })
            .collect();
        UserProfile {
            hot_words,
            correction_patterns,
            ..UserProfile::default()
        }
    }

    fn replay_profile() -> UserProfile {
        let hot_words = vec![
            learned("claude"),
            user("openclaw"),
            learned("README"),
            learned("Windows"),
            learned("Python"),
            learned("OpenAI"),
            learned("Codex"),
            user("github"),
            learned("GPT Pro"),
            user("宇树"),
            user("同济"),
            learned("OAuth"),
            learned("xLSTM"),
            learned("LaTeX"),
            learned("智谱"),
            user("agent"),
        ];
        let correction_patterns = vec![
            correction("Cloud", "claude", 99, CorrectionSource::User),
            correction("open cloud", "openclaw", 33, CorrectionSource::User),
            correction("readme", "README", 19, CorrectionSource::Ai),
            correction("read me", "README", 13, CorrectionSource::Ai),
            correction("Claude", "claude", 10, CorrectionSource::User),
            correction("codeex", "Codex", 10, CorrectionSource::Ai),
            correction("语音", "宇树", 10, CorrectionSource::User),
            correction("github", "GitHub", 9, CorrectionSource::Ai),
            correction("GPT pro", "GPT Pro", 8, CorrectionSource::Ai),
            correction("统计", "同济", 7, CorrectionSource::User),
            correction("gitthub", "GitHub", 4, CorrectionSource::Ai),
            correction("OAUTH", "OAuth", 4, CorrectionSource::Ai),
            correction("XLSTM", "xLSTM", 3, CorrectionSource::Ai),
            correction("gitub", "GitHub", 3, CorrectionSource::Ai),
            correction("LATX", "LaTeX", 3, CorrectionSource::Ai),
            correction("智朴", "智谱", 2, CorrectionSource::Ai),
            correction("A正", "agent", 2, CorrectionSource::Ai),
        ];
        UserProfile {
            hot_words,
            correction_patterns,
            ..UserProfile::default()
        }
    }

    #[test]
    fn manual_vocabulary_preserves_symbolic_names_and_unicode_spelling() {
        for (term, raw, expected) in [
            ("C++", "Use C++ today.", "Use C++ today."),
            ("C++", "Use c++ today.", "Use C++ today."),
            ("C++", "Use C today.", "Use C today."),
            ("C#", "Use c# today.", "Use C# today."),
            ("José", "Ask josé today.", "Ask José today."),
            ("München", "Visit münchen today.", "Visit München today."),
            ("C#", "Use abc# today.", "Use abc# today."),
            ("CAF", "Visit the café.", "Visit the café."),
            (".NET", "Use .net today.", "Use .NET today."),
            (
                "Speak-Flow",
                "Use speak flow today.",
                "Use Speak-Flow today.",
            ),
        ] {
            assert_eq!(
                correct_qwen_hot_words(raw, &[user(term)]).text,
                expected,
                "{term}: {raw}"
            );
        }
    }

    #[test]
    fn manual_vocabulary_is_not_dropped_by_the_learned_word_limit() {
        let mut words: Vec<_> = (0..MAX_ASR_HOT_WORDS)
            .map(|index| hot_word(&format!("Learned{index}"), HotWordSource::Learned, 5, 100))
            .collect();
        words.push(user("SpeakFlow"));
        assert_eq!(
            correct_qwen_hot_words("speak flow", &words).text,
            "SpeakFlow"
        );
    }

    #[test]
    fn corrects_representative_ascii_and_chinese_variants() {
        let hot_words = representative_hot_words();
        let cases = [
            ("我们用 get hub 管理代码。", "我们用 github 管理代码。"),
            ("请用 Cloud Code 写程序。", "请用 claude code 写程序。"),
            ("我戴的是 Aura Ring。", "我戴的是 Oura Ring。"),
            ("请打开 open claw。", "请打开 openclaw。"),
            ("这个工具叫 type less。", "这个工具叫 typeless。"),
            ("设备来自嘉明。", "设备来自佳明。"),
            ("毕业于同机大学。", "毕业于同济大学。"),
            ("打开划词住手。", "打开划词助手。"),
            ("按第一性圆理分析。", "按第一性原理分析。"),
            ("在 vs code 里打开。", "在 VS Code 里打开。"),
        ];

        let corrected = cases
            .iter()
            .filter(|(raw, expected)| correct_qwen_hot_words(raw, &hot_words).text == *expected)
            .count();
        assert_eq!(corrected, cases.len());
    }

    #[test]
    fn leaves_ambiguous_inflected_and_unrelated_text_unchanged() {
        let hot_words = representative_hot_words();
        let cases = [
            "我今天吃了青鱼。",
            "The agents finished their work.",
            "Upload the file to the cloud.",
            "这次欧盟会议按时开始。",
            "正常句子里没有需要替换的术语。",
            "Cloud storage is available.",
        ];

        for raw in cases {
            let result = correct_qwen_hot_words(raw, &hot_words);
            assert_eq!(result.text, raw);
            assert_eq!(result.replacements, 0);
        }
    }

    #[test]
    fn learned_canonical_ascii_terms_require_exact_normalized_matches() {
        let cases = [
            ("OpenAI", "openai", "OpenAI", 1),
            ("OpenAI", "open ai", "OpenAI", 1),
            ("OpenAI", "opena1", "opena1", 0),
            ("VS Code", "vscode", "VS Code", 1),
            ("VS Code", "vs   code", "VS Code", 1),
            ("VS Code", "vs coda", "vs coda", 0),
            ("AI", "a i", "AI", 1),
            ("AI", "aj", "aj", 0),
        ];
        for (hot_text, raw, expected, replacements) in cases {
            let result = correct_qwen_hot_words(raw, &[learned(hot_text)]);
            assert_eq!(result.text, expected, "{hot_text:?}: {raw:?}");
            assert_eq!(result.replacements, replacements, "{hot_text:?}: {raw:?}");
        }
    }

    #[test]
    fn manual_ascii_edit_distance_limit_changes_at_ten_characters() {
        let cases = [
            ("agent", "agxnt", "agxnx"),
            ("github", "gitxub", "gxtxub"),
            ("whisper", "whispex", "whispxx"),
            ("openclaw", "openclax", "openclxx"),
            ("whisperer", "whisperxr", "whispexxr"),
            ("typescript", "typexcripx", "xypexcripx"),
            ("transcription", "xranscriptiox", "xxanscriptiox"),
        ];
        for (hot_text, accepted, rejected) in cases {
            for (raw, expected, replacements) in [(accepted, hot_text, 1), (rejected, rejected, 0)]
            {
                let result = correct_qwen_hot_words(raw, &[user(hot_text)]);
                assert_eq!(result.text, expected, "{hot_text:?}: {raw:?}");
                assert_eq!(result.replacements, replacements, "{hot_text:?}: {raw:?}");
            }
        }
    }

    #[test]
    fn manual_ascii_length_deltas_respect_edit_distance_limits() {
        let cases = [
            ("agent", "agxent", "agent", 1),
            ("agent", "aget", "agent", 1),
            ("agent", "agxyent", "agxyent", 0),
            ("agent", "agn", "agn", 0),
            ("typescript", "typexxscript", "typescript", 1),
            ("typescript", "typscrit", "typescript", 1),
            ("typescript", "typexxxscript", "typexxxscript", 0),
            ("typescript", "typscri", "typscri", 0),
        ];
        for (hot_text, raw, expected, replacements) in cases {
            let result = correct_qwen_hot_words(raw, &[user(hot_text)]);
            assert_eq!(result.text, expected, "{hot_text:?}: {raw:?}");
            assert_eq!(result.replacements, replacements, "{hot_text:?}: {raw:?}");
        }
    }

    #[test]
    fn manual_ascii_simple_inflections_remain_unchanged() {
        let cases = [
            ("agent", "AGENTS"),
            ("agents", "AGENT"),
            ("microbatch", "MICROBATCHES"),
            ("microbatches", "MICROBATCH"),
            ("reconsider", "RECONSIDERED"),
            ("reconsidered", "RECONSIDER"),
            ("process", "PROCESSING"),
            ("processing", "PROCESS"),
        ];
        for (hot_text, raw) in cases {
            let result = correct_qwen_hot_words(raw, &[user(hot_text)]);
            assert_eq!(result.text, raw, "{hot_text:?}: {raw:?}");
            assert_eq!(result.replacements, 0, "{hot_text:?}: {raw:?}");
        }
    }

    #[test]
    fn manual_ascii_terms_shorter_than_five_require_exact_normalized_matches() {
        let cases = [
            ("code", "CODE", "code", 1),
            ("code", "coda", "coda", 0),
            ("AI", "ai", "AI", 1),
            ("AI", "aj", "aj", 0),
        ];
        for (hot_text, raw, expected, replacements) in cases {
            let result = correct_qwen_hot_words(raw, &[user(hot_text)]);
            assert_eq!(result.text, expected, "{hot_text:?}: {raw:?}");
            assert_eq!(result.replacements, replacements, "{hot_text:?}: {raw:?}");
        }
    }

    #[test]
    fn prefers_longer_overlapping_hot_word() {
        let hot_words = vec![user("claude"), user("claude code")];
        let result = correct_qwen_hot_words("Cloud Code is ready.", &hot_words);
        assert_eq!(result.text, "claude code is ready.");
        assert_eq!(result.replacements, 1);
    }

    #[test]
    fn replays_safe_profile_aliases_without_ambiguous_user_rules() {
        let profile = replay_profile();
        let cases = [
            ("open cloud", "openclaw"),
            ("readme", "README"),
            ("read me", "README"),
            ("Claude", "claude"),
            ("codeex", "Codex"),
            ("github", "GitHub"),
            ("GPT pro", "GPT Pro"),
            ("gitthub", "GitHub"),
            ("OAUTH", "OAuth"),
            ("XLSTM", "xLSTM"),
            ("gitub", "GitHub"),
            ("LATX", "LaTeX"),
            ("智朴", "智谱"),
        ];
        for (raw, expected) in cases {
            assert_eq!(correct_qwen_profile_terms(raw, &profile).text, expected);
        }

        for unchanged in [
            "Cloud storage is available.",
            "语音助手已经打开。",
            "统计结果已经完成。",
            "渲染任务已经完成。",
            "A正不是一个确定的术语。",
            "readmefile",
            "mygithubrepo",
        ] {
            assert_eq!(
                correct_qwen_profile_terms(unchanged, &profile).text,
                unchanged
            );
        }
    }

    #[test]
    fn profile_aliases_are_ranked_before_the_one_hundred_entry_limit() {
        let profile = UserProfile {
            hot_words: vec![learned("canonical")],
            correction_patterns: (0..101)
                .rev()
                .map(|index| {
                    correction(
                        &format!(" misheard{index:03} "),
                        "canonical",
                        101 - index,
                        CorrectionSource::Ai,
                    )
                })
                .collect(),
            ..UserProfile::default()
        };
        for (raw, expected, replacements) in [
            ("misheard100", "misheard100", 0),
            ("misheard000", "canonical", 1),
        ] {
            let result = correct_qwen_profile_terms(raw, &profile);
            assert_eq!(result.text, expected, "{raw:?}");
            assert_eq!(result.replacements, replacements, "{raw:?}");
        }
    }

    #[test]
    fn profile_aliases_require_a_matching_hot_word_target() {
        let profile = UserProfile {
            hot_words: vec![learned("canonical")],
            correction_patterns: vec![correction(
                "misheard",
                "absenttarget",
                100,
                CorrectionSource::Ai,
            )],
            ..UserProfile::default()
        };
        let result = correct_qwen_profile_terms("misheard", &profile);
        assert_eq!(result.text, "misheard");
        assert_eq!(result.replacements, 0);
    }

    #[test]
    fn profile_aliases_ignore_empty_and_whitespace_originals() {
        for original in ["", " \t\n "] {
            let profile = UserProfile {
                hot_words: vec![learned("canonical")],
                correction_patterns: vec![correction(
                    original,
                    "canonical",
                    100,
                    CorrectionSource::Ai,
                )],
                ..UserProfile::default()
            };
            let result = correct_qwen_profile_terms("unrelated speech", &profile);
            assert_eq!(result.text, "unrelated speech", "{original:?}");
            assert_eq!(result.replacements, 0, "{original:?}");
        }
    }

    #[test]
    fn real_profile_replay_coverage_improves_without_unsafe_aliases() {
        let profile = replay_profile();
        let cases = [
            ("Cloud", "claude", 99_u32),
            ("open cloud", "openclaw", 33),
            ("readme", "README", 19),
            ("read me", "README", 13),
            ("Claude", "claude", 10),
            ("codeex", "Codex", 10),
            ("语音", "宇树", 10),
            ("github", "GitHub", 9),
            ("GPT pro", "GPT Pro", 8),
            ("统计", "同济", 7),
            ("gitthub", "GitHub", 4),
            ("OAUTH", "OAuth", 4),
            ("XLSTM", "xLSTM", 3),
            ("gitub", "GitHub", 3),
            ("LATX", "LaTeX", 3),
            ("智朴", "智谱", 2),
            ("A正", "agent", 2),
        ];
        let total: u32 = cases.iter().map(|(_, _, count)| count).sum();
        let before: u32 = cases
            .iter()
            .filter(|(raw, expected, _)| {
                correct_qwen_hot_words(raw, &profile.hot_words).text == *expected
            })
            .map(|(_, _, count)| count)
            .sum();
        let after: u32 = cases
            .iter()
            .filter(|(raw, expected, _)| {
                correct_qwen_profile_terms(raw, &profile).text == *expected
            })
            .map(|(_, _, count)| count)
            .sum();

        println!(
            "SPEAK_FLOW_ALIAS_METRICS before={before}/{total} after={after}/{total} delta_events={}",
            after - before
        );
        assert_eq!(before, 47);
        assert_eq!(after, 121);
    }

    #[test]
    fn corrects_misheard_occurrences_even_when_hotword_already_present() {
        let hot_words = vec![user("github")];
        let result = correct_qwen_hot_words(
            "I already use github, but get hub is misspelled.",
            &hot_words,
        );
        assert_eq!(
            result.text,
            "I already use github, but github is misspelled."
        );
        assert_eq!(result.replacements, 1);
    }

    #[test]
    fn manual_terms_match_similar_sounding_spellings() {
        for (term, raw, expected) in [
            ("Qwen", "Switch to quen today.", "Switch to Qwen today."),
            ("nginx", "Restart engine x now.", "Restart nginx now."),
            ("Phoenix", "Deploy fenix later.", "Deploy Phoenix later."),
            ("Khent", "Ask Kent about it.", "Ask Khent about it."),
        ] {
            assert_eq!(
                correct_qwen_hot_words(raw, &[user(term)]).text,
                expected,
                "{term}: {raw}"
            );
        }
    }

    #[test]
    fn sound_matching_leaves_everyday_words_and_learned_terms_alone() {
        for (hot_word, raw) in [
            (user("Nuxt"), "Do the next step."),
            (user("Khent"), "Count the files."),
            (user("Tauri"), "That is true."),
            (learned("Qwen"), "Switch to quen today."),
        ] {
            let result = correct_qwen_hot_words(raw, std::slice::from_ref(&hot_word));
            assert_eq!(result.text, raw, "{}: {raw}", hot_word.text);
            assert_eq!(result.replacements, 0, "{}: {raw}", hot_word.text);
        }
    }

    #[test]
    fn sound_keys_merge_spellings_that_sound_alike() {
        for (left, right) in [
            ("qwen", "quen"),
            ("phoenix", "fenix"),
            ("nginx", "enginex"),
            ("knight", "nite"),
            ("cent", "sent"),
        ] {
            assert_eq!(sound_key(left), sound_key(right), "{left} / {right}");
        }
        assert_ne!(sound_key("github"), sound_key("gxtxub"));
    }

    #[test]
    fn common_english_words_are_sorted_and_unique() {
        assert!(COMMON_ENGLISH_WORDS
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
    }

    fn manual_rule(original: &str, corrected: &str) -> CorrectionPattern {
        CorrectionPattern {
            manual: true,
            ..correction(original, corrected, 3, CorrectionSource::User)
        }
    }

    #[test]
    fn manual_rules_apply_without_a_vocabulary_target_and_ignore_case() {
        let profile = UserProfile {
            correction_patterns: vec![
                manual_rule("Tora", "Tauri"),
                manual_rule("Cloud", "claude"),
                manual_rule("语音", "宇树"),
            ],
            ..UserProfile::default()
        };
        for (raw, expected) in [
            ("Build it with tora.", "Build it with Tauri."),
            ("TORA is fast.", "Tauri is fast."),
            ("Ask Cloud.", "Ask claude."),
            ("打开语音。", "打开宇树。"),
            ("The torah scroll.", "The torah scroll."),
        ] {
            assert_eq!(
                correct_qwen_profile_terms(raw, &profile).text,
                expected,
                "{raw}"
            );
        }
    }

    #[test]
    fn learned_user_rules_keep_the_single_word_safety_limit() {
        let profile = UserProfile {
            hot_words: vec![learned("Tauri")],
            correction_patterns: vec![correction("Tora", "Tauri", 3, CorrectionSource::User)],
            ..UserProfile::default()
        };
        assert_eq!(
            correct_qwen_profile_terms("Build it with Tora.", &profile).text,
            "Build it with Tora."
        );
    }

    #[test]
    fn manual_rules_rank_ahead_of_the_alias_limit() {
        let mut correction_patterns: Vec<_> = (0..MAX_ASR_ALIASES)
            .map(|index| {
                correction(
                    &format!("misheard{index:03}"),
                    "canonical",
                    1_000,
                    CorrectionSource::Ai,
                )
            })
            .collect();
        correction_patterns.push(manual_rule("Tora", "Tauri"));
        let profile = UserProfile {
            hot_words: vec![learned("canonical")],
            correction_patterns,
            ..UserProfile::default()
        };
        assert_eq!(correct_qwen_profile_terms("Tora", &profile).text, "Tauri");
    }

    #[test]
    fn hotword_correction_p95_stays_below_one_millisecond() {
        let profile = maximum_profile_load();
        let text = "请在 Cloud Code 和 get hub 中检查同机大学的项目，然后打开划词住手。";
        for _ in 0..100 {
            let _ = correct_qwen_profile_terms(text, &profile);
        }

        let mut samples = Vec::with_capacity(1000);
        for _ in 0..1000 {
            let started = Instant::now();
            let result = correct_qwen_profile_terms(text, &profile);
            assert_eq!(result.replacements, 4);
            samples.push(started.elapsed().as_micros());
        }
        samples.sort_unstable();
        let p95_us = samples[949];
        println!("SPEAK_FLOW_HOTWORD_METRICS coverage=100 false_positive=0 p95_us={p95_us}");
        assert!(p95_us < 1000, "hotword correction p95 was {p95_us} us");
    }
}
