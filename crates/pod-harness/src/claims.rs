//! The documentation claim check of CLAIM-1 to CLAIM-4.
//!
//! A claim needs its conditions: which host, which version, which input and
//! how many runs. A comparative needs a number and a date near it. A claim
//! that holds neither is removed, not softened.

use crate::date::is_iso_date;

/// The comparative words of CLAIM-4.
pub const COMPARATIVES: [&str; 4] = ["faster", "more secure", "robust", "better"];

/// The words that name a condition of a measurement.
pub const CONDITIONS: [&str; 10] =
    ["host", "version", "input", "run", "runs", "measured", "fixture", "platform", "build", "sample"];

/// The units of a measurement. A word after a number from this list makes
/// the line a claim that needs its conditions.
pub const UNITS: [&str; 11] = ["s", "ms", "us", "min", "minutes", "%", "MiB", "MB", "GiB", "GB", "runs"];

/// One fault of a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The line of the text, from 1.
    pub line: usize,
    /// The rule: `bare-comparative` or `no-conditions`.
    pub rule: &'static str,
    /// The line, trimmed.
    pub text: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}: {}", self.line, self.rule, self.text)
    }
}

/// Each finding of `text`. An empty result is a text that passes. A word
/// in inline code or in a fenced code block is a quotation, not a claim.
pub fn check_claims(text: &str) -> Vec<Finding> {
    let mut clean = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            clean.push(String::new());
        } else if fenced {
            clean.push(String::new());
        } else {
            clean.push(strip_code(line));
        }
    }
    let mut out = Vec::new();
    for (index, line) in clean.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || trimmed.starts_with('|') {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let comparative = COMPARATIVES.iter().find(|word| holds_word(&lower, word)).copied();
        let measurement = is_measurement(line);
        if comparative.is_none() && !measurement {
            continue;
        }
        // The conditions can be on the line before or after the claim.
        let start = index.saturating_sub(2);
        let end = (index + 3).min(clean.len());
        let window = clean[start..end].join(" ");
        let number = has_number(&window);
        let date = has_date(&window);
        let conditions = has_conditions(&window);
        let rule = match comparative {
            Some(word) if !number || !date => Some(("bare-comparative", word)),
            Some(word) if !conditions => Some(("no-conditions", word)),
            Some(_) => None,
            None if !conditions => Some(("no-conditions", "measurement")),
            None => None,
        };
        if let Some((rule, word)) = rule {
            out.push(Finding { line: index + 1, rule, text: format!("{} ({word})", line.trim()) });
        }
    }
    out
}

fn is_measurement(line: &str) -> bool {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let token = token.trim_matches(|c: char| c.is_ascii_punctuation() && c != '.' && c != '%');
        if token.ends_with('%') && token[..token.len() - 1].parse::<f64>().is_ok() {
            return true;
        }
        if UNITS.iter().any(|unit| {
            let Some(prefix) = token.strip_suffix(unit) else { return false };
            !prefix.is_empty() && prefix.parse::<f64>().is_ok()
        }) {
            return true;
        }
        if token.parse::<f64>().is_ok()
            && let Some(next) = tokens.get(index + 1)
        {
            let next = next.trim_matches(|c: char| c.is_ascii_punctuation() && c != '%');
            if UNITS.contains(&next) {
                return true;
            }
        }
    }
    false
}

/// A line with each inline code segment removed.
fn strip_code(line: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in line.chars() {
        if c == '`' {
            inside = !inside;
        } else if !inside {
            out.push(c);
        }
    }
    out
}

fn holds_word(text: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_ascii_alphanumeric());
        if boundary(before) && boundary(after) {
            return true;
        }
        from = end;
    }
    false
}

/// A number that is not a part of a date.
fn has_number(text: &str) -> bool {
    let mut at = 0;
    let bytes = text.as_bytes();
    while at < bytes.len() {
        if bytes[at].is_ascii_digit() {
            let end = (at + 10).min(bytes.len());
            if end - at == 10 && is_iso_date(&text[at..end]) {
                at = end;
                continue;
            }
            return true;
        }
        at += 1;
    }
    false
}

fn has_date(text: &str) -> bool {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '(' || c == ')')
        .any(|token| is_iso_date(token.trim_end_matches('.')))
}

fn has_conditions(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).collect();
    CONDITIONS.iter().any(|condition| tokens.contains(condition))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_comparative_fails() {
        for text in
            ["The tunnel is faster.", "This road is more secure.", "The padding is robust.", "This profile is better."]
        {
            let findings = check_claims(text);
            assert_eq!(findings.len(), 1, "{text}: {findings:?}");
            assert_eq!(findings[0].rule, "bare-comparative", "{text}");
        }
    }

    #[test]
    fn a_claim_with_a_number_a_date_and_conditions_passes() {
        let text = "The padded road is 0.21 less visible than the unpadded road on 2026-11-20; \
                    host linux, version 1.0, input fixtures/tunnel.cap, 3 runs.";
        assert_eq!(check_claims(text), Vec::new());
    }

    #[test]
    fn a_claim_with_a_number_and_a_date_but_no_conditions_fails() {
        let text = "The padded road is better: 0.21 against 0.55 on 2026-11-20.";
        let findings = check_claims(text);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "no-conditions");
    }

    #[test]
    fn a_text_without_a_comparative_passes() {
        let text = "The harness reads no network.\nThe table has a row for each road.";
        assert_eq!(check_claims(text), Vec::new());
    }

    #[test]
    fn a_date_does_not_count_as_the_number() {
        let text = "This road is faster on 2026-11-20.";
        assert_eq!(check_claims(text)[0].rule, "bare-comparative");
    }

    #[test]
    fn a_comparative_inside_another_word_does_not_count() {
        let text = "The betters are not compared.";
        assert_eq!(check_claims(text), Vec::new());
    }

    #[test]
    fn a_measurement_without_conditions_fails() {
        let findings = check_claims("The harness takes 10 minutes.");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "no-conditions");
        let ok = "The harness takes 10 minutes on host linux, version 1.0, input fixtures, 3 runs.";
        assert_eq!(check_claims(ok), Vec::new());
    }

    #[test]
    fn a_table_row_and_a_heading_are_not_claims() {
        let text = "# The 10 minutes\n| row | 10 minutes |\n";
        assert_eq!(check_claims(text), Vec::new());
    }

    #[test]
    fn a_word_in_code_is_a_quotation() {
        let text = "The labels `faster` and `better` are read from a list.\n\n```\nThis road is faster.\n```\n";
        assert_eq!(check_claims(text), Vec::new());
    }
}
