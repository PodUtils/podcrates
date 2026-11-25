//! The line of the error of a name that does not resolve: one phrase for
//! each source, in a bound of bytes. A phrase keeps its parts, so that the
//! bound drops words of a cause at a part boundary and does not cut inside
//! the text of a cause (PodUtils/podnet#753).

use std::fmt;

/// The failure of one source: its lead (the source, the count of the
/// resolvers) and its causes, each with its own lead (`4 with `).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Phrase {
    lead: String,
    causes: Vec<Cause>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cause {
    lead: String,
    /// The text in the line: the whole cause, or a brief form of it.
    text: String,
    whole: String,
}

impl Phrase {
    /// A phrase of `lead` and its causes, each `(lead, text)`.
    pub(crate) fn new(lead: impl Into<String>, causes: Vec<(String, String)>) -> Phrase {
        let causes = causes.into_iter().map(|(lead, text)| Cause { lead, whole: text.clone(), text }).collect();
        Phrase { lead: lead.into(), causes }
    }

    /// A phrase of one cause and no lead.
    pub(crate) fn of(text: impl Into<String>) -> Phrase {
        Phrase::new("", vec![(String::new(), text.into())])
    }

    /// The phrase with `lead` (the name of a source) before its own lead.
    pub(crate) fn led(mut self, lead: &str) -> Phrase {
        self.lead.insert_str(0, lead);
        self
    }
}

impl fmt::Display for Phrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.lead)?;
        for (i, cause) in self.causes.iter().enumerate() {
            let sep = if i == 0 { "" } else { "; " };
            write!(f, "{sep}{}{}", cause.lead, cause.text)?;
        }
        Ok(())
    }
}

impl From<String> for Phrase {
    fn from(text: String) -> Phrase {
        Phrase::of(text)
    }
}

impl From<&str> for Phrase {
    fn from(text: &str) -> Phrase {
        Phrase::of(text)
    }
}

/// The brief forms of a cause, in turn, each of whole words at a part
/// boundary: the first sentence (a hint or a list of names after `; `
/// goes); its head and its last part (`TLS: invalid peer certificate:
/// UnknownIssuer` is `TLS: UnknownIssuer`); its last part, the words of the
/// system (`Network is unreachable`); its head, the class of the cause
/// (`TLS`, `could not connect`).
const STEPS: usize = 4;

/// The form `step` of the cause `whole`, when it has one.
fn form(whole: &str, step: usize) -> Option<String> {
    let first = whole.split_once("; ").map_or(whole, |(first, _)| first);
    let parts: Vec<&str> = first.split(": ").collect();
    let (head, last) = (parts[0], parts[parts.len() - 1]);
    let out = match step {
        0 => first.to_string(),
        1 if parts.len() > 2 => format!("{head}: {last}"),
        2 if parts.len() > 1 => last.to_string(),
        3 if parts.len() > 1 => head.to_string(),
        _ => return None,
    };
    (!head.is_empty() && !last.is_empty()).then_some(out)
}

/// The form `step` of `cause`, when it is shorter than its text now.
fn brief(cause: &Cause, step: usize) -> Option<String> {
    form(&cause.whole, step).filter(|out| out.len() < cause.text.len())
}

fn join(phrases: &[Phrase]) -> String {
    phrases.iter().map(Phrase::to_string).collect::<Vec<_>>().join("; ")
}

/// The place of the longest cause that `can` takes, the first of two of
/// the same length: its phrase and its place in the phrase.
fn longest(phrases: &[Phrase], can: impl Fn(&Cause) -> bool) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize, usize)> = None;
    for (p, phrase) in phrases.iter().enumerate() {
        for (c, cause) in phrase.causes.iter().enumerate() {
            if best.is_none_or(|(_, _, len)| cause.text.len() > len) && can(cause) {
                best = Some((p, c, cause.text.len()));
            }
        }
    }
    best.map(|(p, c, _)| (p, c))
}

/// The phrase of each source, joined with `; `, in at most `most` bytes.
/// Each lead stays, so that each source and each group of resolvers keeps
/// its name. The causes become brief in steps ([`STEPS`]), the longest
/// cause first in each step, until the line fits. Only a line whose leads
/// and classes do not fit is cut inside a cause, as the last resort. The
/// whole line goes to the log at debug.
pub(crate) fn fit(mut phrases: Vec<Phrase>, most: usize) -> String {
    let whole = join(&phrases);
    if whole.len() <= most {
        return whole;
    }
    tracing::debug!(failures = %whole, "each source of the addresses failed");
    for step in 0..STEPS {
        loop {
            let line = join(&phrases);
            if line.len() <= most {
                return line;
            }
            let Some((p, c)) = longest(&phrases, |cause| brief(cause, step).is_some()) else { break };
            let cause = &mut phrases[p].causes[c];
            cause.text = brief(cause, step).unwrap_or_default();
        }
    }
    loop {
        let line = join(&phrases);
        let over = line.len().saturating_sub(most);
        if over == 0 {
            return line;
        }
        let Some((p, c)) = longest(&phrases, |cause| cause.text.len() > 32) else {
            return pod_core::text::clip(&line, most);
        };
        let cause = &mut phrases[p].causes[c].text;
        *cause = pod_core::text::clip(cause, cause.len().saturating_sub(over).max(32));
    }
}

#[cfg(test)]
mod tests;
