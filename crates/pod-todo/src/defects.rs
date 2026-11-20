//! The rule that a done defect names its test (`defect_tests` of
//! `TODO/todo.toml`): an entry of the category `defect` that is `done`, and
//! whose first date in `## Done` is on or after the date of the key, names a
//! test in its `## Done`. A test is a function in a code span (a snake_case
//! word with two `_` or more, alone or after a path or a module path), or a
//! path under `tests/`, `scripts/` or `.github/workflows/`. These are the
//! forms of podnet's expression, so that its counts hold here:
//! ``(tests|scripts|\.github/workflows)/|`[^`\s]*\b[a-z][a-z0-9]*(_[a-z0-9]+){2,}` ``.

use crate::model::Entry;
use crate::parse::code_spans;

/// The words of the problem, after `T-NNN: `.
pub const NO_TEST: &str = "a done defect names no test in its Done section";

/// The line of `## Done` and the words of the problem, when the rule of the
/// configuration applies to `e` with the status `status` (its own, or the
/// one that `set` writes) and its Done section names no test.
pub fn gap(e: &Entry, status: &str) -> Option<(usize, String)> {
    let config = crate::config::current();
    let since = config.defect_tests.as_deref()?;
    if e.field("Category") != Some("defect") || status != "done" {
        return None;
    }
    let done = e.section("Done")?;
    // A Done with no date fails a rule of its own.
    let date = crate::check::first_date(&done.body)?;
    (date >= since && !names_test(&done.body)).then(|| (done.line, NO_TEST.to_string()))
}

/// True when a text names a test in one of the two forms.
pub fn names_test(text: &str) -> bool {
    ["tests/", "scripts/", ".github/workflows/"].iter().any(|dir| text.contains(dir))
        || text.lines().flat_map(code_spans).any(ends_with_test_name)
}

/// A code span with no space whose last word is a test function: `a_b_c`,
/// `x::a_b_c`, `tests/x.rs::a_b_c`. The last word is the run of word
/// characters (`[A-Za-z0-9_]`) at the end of the span.
fn ends_with_test_name(span: &str) -> bool {
    if span.is_empty() || span.contains(char::is_whitespace) {
        return false;
    }
    let start = span
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let word = &span[start..];
    let mut parts = word.split('_');
    let first = parts.next().unwrap_or("");
    let lower = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    first.starts_with(|c: char| c.is_ascii_lowercase())
        && lower(first)
        && word.matches('_').count() >= 2
        && parts.all(lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_function_is_a_word_with_two_underscores_at_the_end_of_a_span() {
        for yes in [
            "`a_b_c`",
            "`tests/x.rs` passed",
            "`cli_tests::a_b_c`",
            "`crates/x/src/lib.rs::the_fix_holds_here`",
            "the step of .github/workflows/ci.yml",
            "`sh scripts/x.sh`",
            "two `x` and `y::one_two_3`",
            "`é::a_b_c`",
        ] {
            assert!(names_test(yes), "{yes}");
        }
        for no in [
            "no span: a_b_c",
            "`a_b`",
            "`A_b_c`",
            "`a__b_c`",
            "`a_b_c_`",
            "`a_b_c()`",
            "`cargo test a_b_c`",
            "`x::Ab_cd_ef`",
            "`_a_b_c`",
            "",
        ] {
            assert!(!names_test(no), "{no}");
        }
    }
}
