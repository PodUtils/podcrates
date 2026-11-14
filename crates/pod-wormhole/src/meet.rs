//! Where the two ends of a code meet. The giver allocates a nameplate at the
//! first mailbox that answers, makes the code, and waits for one taker. The
//! taker looks for its giver at each mailbox in order: one that lists the
//! code's nameplate has it, one that lists others does not, and at one that
//! lists none the giver's `pake`, which the mailbox keeps, comes at once or
//! not at all. What the ends say once they hold a key is the caller's: the
//! exchange is a protocol over a [`Wormhole`].

use std::time::Duration;

use pod_core::Event;
use pod_core::entropy::{Entropy, OsEntropy};
use pod_ws::url::WsUrl;
use serde_json::Value;

use crate::config::Config;
use crate::mailbox::{Mailbox, Mood, crowded};
use crate::message::hex;
use crate::phrase::check_words;
use crate::wormhole::Wormhole;
use crate::{Code, Failed};

/// Where the ends meet, and how they are reached.
pub struct Ends {
    /// The mailboxes, in order.
    pub mailboxes: Vec<WsUrl>,
    pub config: Config,
}

/// A new code at the first mailbox of `ends` that answers, and its
/// wormhole, this end's `pake` added: what a giver shows. `versions` goes in
/// its version phase. A count of words out of its bounds
/// ([`crate::phrase::check_words`]) fails before a mailbox is reached.
pub async fn opened(ends: &Ends, versions: Value) -> Result<(Wormhole, Code), Failed> {
    check_words(ends.config.words)?;
    let side = new_side()?;
    let mut missed = Vec::new();
    for url in &ends.mailboxes {
        let mut mailbox = match Mailbox::connect(url, &ends.config, &side).await {
            Ok(mailbox) => mailbox,
            Err(why) => {
                say(ends, Event::warn("wormhole.mailbox", reason(&why)));
                missed.push(reason(&why));
                continue;
            }
        };
        say_motd(ends, &mailbox);
        let claimed = match mailbox.allocate().await {
            Ok(nameplate) => mailbox.claim(&nameplate).await.map(|id| (nameplate, id)),
            Err(why) => Err(why),
        };
        let (nameplate, id) = match claimed {
            Ok(claimed) => claimed,
            Err(why) => {
                mailbox.end().await;
                say(ends, Event::warn("wormhole.mailbox", reason(&why)));
                missed.push(reason(&why));
                continue;
            }
        };
        let code = Code::make(&nameplate, ends.config.words, &mut OsEntropy)?;
        let hole = Wormhole::open(mailbox, side, nameplate, id, &code, versions).await?;
        return Ok((hole, code));
    }
    Err(Failed::Unreachable(missed.join("; ")))
}

/// The wormhole of `code` at the mailbox where its giver waits, the key
/// made: each mailbox of `ends` in order, by its list of nameplates, and
/// where it lists none, by the giver's `pake` within `giver`.
pub async fn joined(ends: &Ends, code: &Code, versions: Value, giver: Duration) -> Result<Wormhole, Failed> {
    let side = new_side()?;
    let (mut missed, mut reached) = (Vec::new(), false);
    for url in &ends.mailboxes {
        let mut mailbox = match Mailbox::connect(url, &ends.config, &side).await {
            Ok(mailbox) => mailbox,
            Err(why) => {
                say(ends, Event::warn("wormhole.mailbox", reason(&why)));
                missed.push(reason(&why));
                continue;
            }
        };
        say_motd(ends, &mailbox);
        let listed = match mailbox.list().await {
            Ok(listed) => listed,
            Err(why) => {
                mailbox.end().await;
                say(ends, Event::warn("wormhole.mailbox", reason(&why)));
                missed.push(reason(&why));
                continue;
            }
        };
        reached = true;
        // A mailbox that lists others has no giver of this code: claiming
        // the nameplate there would spend a stranger's code.
        if !listed.is_empty() && !listed.iter().any(|nameplate| nameplate == code.nameplate()) {
            mailbox.end().await;
            continue;
        }
        let id = match mailbox.claim(code.nameplate()).await {
            Ok(id) => id,
            Err(why) if crowded(&why) => {
                mailbox.end().await;
                let shown = &mailbox.shown;
                return Err(Failed::Refused(format!("two ends hold this code at {shown} already: it was taken")));
            }
            Err(why) => {
                mailbox.end().await;
                say(ends, Event::warn("wormhole.mailbox", reason(&why)));
                missed.push(reason(&why));
                continue;
            }
        };
        let nameplate = code.nameplate().to_string();
        let mut hole = Wormhole::open(mailbox, side.clone(), nameplate, id, code, versions.clone()).await?;
        if hole.agree(giver).await? {
            return Ok(hole);
        }
        hole.close(Mood::Lonely).await;
    }
    if reached { Err(Failed::Nobody) } else { Err(Failed::Unreachable(missed.join("; "))) }
}

/// `done`, or its failure after the wormhole closed with `errory`: the
/// close that each failed step of an exchange makes.
pub async fn or_close<T>(hole: &mut Wormhole, done: Result<T, Failed>) -> Result<T, Failed> {
    match done {
        Ok(value) => Ok(value),
        Err(failed) => Err(hole.fail(Mood::Errory, failed).await),
    }
}

/// A side for a run: 10 hex digits, as magic-wormhole's own.
pub(crate) fn new_side() -> Result<String, Failed> {
    let mut bytes = [0u8; 5];
    OsEntropy.fill(&mut bytes).map_err(|_| Failed::NoRandom)?;
    Ok(hex(&bytes))
}

/// The words of a failure at one mailbox, with no prefix of the whole run.
pub(crate) fn reason(failed: &Failed) -> String {
    match failed {
        Failed::Unreachable(why) => why.clone(),
        other => other.to_string(),
    }
}

fn say_motd(ends: &Ends, mailbox: &Mailbox) {
    if let Some(motd) = &mailbox.motd {
        say(ends, Event::info("wormhole.motd", format!("the mailbox {} says: {motd}", mailbox.shown)));
    }
}

fn say(ends: &Ends, event: Event) {
    ends.config.events.event(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_side_is_10_hex_digits_of_its_own() {
        let (one, two) = (new_side().unwrap(), new_side().unwrap());
        assert!(one.len() == 10 && one.bytes().all(|b| b.is_ascii_hexdigit()), "{one}");
        assert_ne!(one, two);
    }

    #[test]
    fn a_failure_at_one_mailbox_is_said_with_no_prefix_of_the_run() {
        assert_eq!(reason(&Failed::Unreachable("wss://m/v1: refused".into())), "wss://m/v1: refused");
        assert_eq!(reason(&Failed::Broken("x".into())), "x");
    }
}
