//! Deterministic, offline cleanup rules. The floor every dictation gets.

use super::{CleanupRequest, DictationStyle, DictionaryEntry, TextCleaner};

pub struct MechanicalCleaner;

const FILLERS: &[&str] = &["uh", "um", "uhm", "erm", "hmm"];

impl TextCleaner for MechanicalCleaner {
    fn clean(&self, request: &CleanupRequest) -> String {
        let mut text = strip_fillers(&request.raw);
        text = apply_dictionary(&text, &request.dictionary);
        text = collapse_whitespace(&text);
        match request.style {
            DictationStyle::CasualLowercase => {
                text = text.to_lowercase();
                // Keep the pronoun "i" natural even in lowercase mode? No:
                // casual lowercase means lowercase; leave as-is.
            }
            DictationStyle::Standard | DictationStyle::Formal => {
                text = capitalize_sentences(&text);
                text = ensure_terminal_punctuation(&text);
                if request.style == DictationStyle::Formal {
                    text = expand_contractions(&text);
                }
            }
        }
        text
    }
}

/// Verbatim cleanup: applies the user dictionary and normalizes whitespace,
/// but never strips fillers, recases, or repunctuates. The `Raw` polish level.
pub struct RawCleaner;

impl TextCleaner for RawCleaner {
    fn clean(&self, request: &CleanupRequest) -> String {
        let text = apply_dictionary(&request.raw, &request.dictionary);
        collapse_whitespace(&text)
    }
}

fn strip_fillers(text: &str) -> String {
    text.split_whitespace()
        .filter(|word| {
            let bare: String = word
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase();
            !FILLERS.contains(&bare.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn apply_dictionary(text: &str, entries: &[DictionaryEntry]) -> String {
    let mut result = text.to_string();
    for entry in entries {
        if entry.pattern.is_empty() {
            continue;
        }
        // Whole-word, case-insensitive replacement without regex: scan words.
        result = result
            .split(' ')
            .map(|word| {
                let (core, trailing) = split_trailing_punct(word);
                if core.eq_ignore_ascii_case(&entry.pattern) {
                    format!("{}{}", entry.replacement, trailing)
                } else {
                    word.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    result
}

fn split_trailing_punct(word: &str) -> (&str, &str) {
    let trimmed = word.trim_end_matches(|c: char| !c.is_alphanumeric());
    (trimmed, &word[trimmed.len()..])
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn capitalize_sentences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut capitalize_next = true;
    for c in text.chars() {
        if capitalize_next && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
            if matches!(c, '.' | '!' | '?') {
                capitalize_next = true;
            }
        }
    }
    out
}

fn ensure_terminal_punctuation(text: &str) -> String {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return String::new();
    }
    let last = trimmed.chars().last().unwrap();
    if matches!(last, '.' | '!' | '?' | ':' | ',' | ';') {
        trimmed.to_string()
    } else {
        format!("{trimmed}.")
    }
}

fn expand_contractions(text: &str) -> String {
    const PAIRS: &[(&str, &str)] = &[
        ("can't", "cannot"),
        ("won't", "will not"),
        ("don't", "do not"),
        ("doesn't", "does not"),
        ("didn't", "did not"),
        ("isn't", "is not"),
        ("aren't", "are not"),
        ("wasn't", "was not"),
        ("weren't", "were not"),
        ("haven't", "have not"),
        ("hasn't", "has not"),
        ("hadn't", "had not"),
        ("shouldn't", "should not"),
        ("wouldn't", "would not"),
        ("couldn't", "could not"),
        ("it's", "it is"),
        ("that's", "that is"),
        ("there's", "there is"),
        ("i'm", "I am"),
        ("i've", "I have"),
        ("i'll", "I will"),
        ("we're", "we are"),
        ("we've", "we have"),
        ("you're", "you are"),
        ("you've", "you have"),
        ("they're", "they are"),
        ("let's", "let us"),
    ];
    text.split(' ')
        .map(|word| {
            let (core, trailing) = split_trailing_punct(word);
            for (from, to) in PAIRS {
                if core.eq_ignore_ascii_case(from) {
                    let expanded = if core.chars().next().is_some_and(|c| c.is_uppercase()) {
                        capitalize_first(to)
                    } else {
                        (*to).to_string()
                    };
                    return format!("{expanded}{trailing}");
                }
            }
            word.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PolishedTone, TargetContext};
    use super::*;

    fn request(raw: &str, style: DictationStyle) -> CleanupRequest {
        CleanupRequest {
            raw: raw.to_string(),
            style,
            tone: PolishedTone::Neutral,
            context: TargetContext::Generic,
            dictionary: vec![],
        }
    }

    #[test]
    fn strips_fillers_and_punctuates() {
        let out = MechanicalCleaner.clean(&request(
            "um so the meeting is uh moved to friday",
            DictationStyle::Standard,
        ));
        assert_eq!(out, "So the meeting is moved to friday.");
    }

    #[test]
    fn casual_lowercase_lowercases_everything() {
        let out = MechanicalCleaner.clean(&request(
            "Send It Tomorrow Morning",
            DictationStyle::CasualLowercase,
        ));
        assert_eq!(out, "send it tomorrow morning");
    }

    #[test]
    fn formal_expands_contractions() {
        let out = MechanicalCleaner.clean(&request(
            "i can't make it, it's too late",
            DictationStyle::Formal,
        ));
        assert_eq!(out, "I cannot make it, it is too late.");
    }

    #[test]
    fn dictionary_replaces_whole_words_keeping_punctuation() {
        let req = CleanupRequest {
            raw: "ping arya about the k8s cluster.".into(),
            style: DictationStyle::Standard,
            tone: PolishedTone::Neutral,
            context: TargetContext::Generic,
            dictionary: vec![DictionaryEntry {
                pattern: "k8s".into(),
                replacement: "Kubernetes".into(),
            }],
        };
        let out = MechanicalCleaner.clean(&req);
        assert_eq!(out, "Ping arya about the Kubernetes cluster.");
    }

    #[test]
    fn existing_terminal_punctuation_is_kept() {
        let out = MechanicalCleaner.clean(&request("is it done?", DictationStyle::Standard));
        assert_eq!(out, "Is it done?");
    }

    #[test]
    fn raw_keeps_words_verbatim_but_applies_dictionary() {
        let req = CleanupRequest {
            raw: "um  send it to arya at 5pm".into(),
            style: DictationStyle::Standard,
            tone: PolishedTone::Neutral,
            context: TargetContext::Generic,
            dictionary: vec![DictionaryEntry {
                pattern: "arya".into(),
                replacement: "Arya".into(),
            }],
        };
        // Filler kept, no capitalization, no terminal period; only the
        // dictionary replacement and whitespace collapse apply.
        assert_eq!(RawCleaner.clean(&req), "um send it to Arya at 5pm");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(
            MechanicalCleaner.clean(&request("", DictationStyle::Standard)),
            ""
        );
    }
}
