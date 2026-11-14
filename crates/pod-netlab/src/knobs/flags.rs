//! The flags of `sand net up`, ported unchanged: a consumer's command line
//! gives them to `Knobs::parse_flags`, and `Knobs::flags` writes a knob
//! set back as flags (a scenario as its command line).

use super::check::{Contradiction, DELAY_K, LOSS_K, MTU_K, RATE_K, Ranged, port_k};
use super::{Broken, Icmp, KnobError, Knobs, Nat, NatFlags, Ping, Policy, Stack, loss_text};

fn bad(flag: &str, why: impl Into<String>) -> KnobError {
    KnobError::BadFlag { flag: flag.to_string(), why: why.into() }
}

/// The refusal of a value with a `-` before its digits: its own text, with
/// the range, as "not a number" would mislead.
fn negative(flag: &str, value: &str, k: &Ranged) -> Option<KnobError> {
    let digits = value.strip_prefix('-')?;
    let digits = digits.split('.').all(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
    digits.then(|| bad(flag, format!("the value {value} is negative; give {} to {}", (k.text)(k.min), (k.text)(k.max))))
}

/// A whole number of the knob `k`, of type `T`. A value out of the range
/// (also one that `u64` cannot hold) names the value as written, with the
/// unit of the knob, and the range.
fn number<T: TryFrom<u64>>(flag: &str, value: &str, k: &Ranged) -> Result<T, KnobError> {
    if let Some(e) = negative(flag, value, k) {
        return Err(e);
    }
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad(flag, format!("the value {value:?} is not a whole number")));
    }
    let n: u64 = value.parse().map_err(|_| k.refuse(value))?;
    if !(k.min..=k.max).contains(&n) {
        return Err(k.refuse(value));
    }
    T::try_from(n).map_err(|_| k.refuse(value))
}

/// A percent with at most two decimals (`5`, `0.25`) in hundredths, in the
/// range of `--loss`.
fn percent(flag: &str, value: &str) -> Result<u16, KnobError> {
    let value = value.strip_suffix('%').unwrap_or(value);
    if let Some(e) = negative(flag, value, &LOSS_K) {
        return Err(e);
    }
    let (whole, frac) = value.split_once('.').unwrap_or((value, ""));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || frac.len() > 2 || !(frac.is_empty() || digits(frac)) {
        return Err(bad(flag, format!("the value {value:?} is not a percent with at most two decimals")));
    }
    let refuse = || LOSS_K.refuse(value);
    let whole: u64 = whole.parse().map_err(|_| refuse())?;
    let frac: u64 = if frac.is_empty() { 0 } else { frac.parse::<u64>().map_err(|_| refuse())? };
    let frac = if value.split_once('.').is_some_and(|(_, f)| f.len() == 1) { frac * 10 } else { frac };
    let bp = whole.checked_mul(100).and_then(|w| w.checked_add(frac)).ok_or_else(refuse)?;
    if !(LOSS_K.min..=LOSS_K.max).contains(&bp) {
        return Err(refuse());
    }
    u16::try_from(bp).map_err(|_| refuse())
}

/// The word of an empty list of `--tcp` or `--udp`: an empty value does
/// not stay a word when a shell splits a command line.
const NO_PORTS: &str = "none";

fn port_list(flag: &str, value: &str) -> Result<Vec<u16>, KnobError> {
    if value.is_empty() || value == NO_PORTS {
        return Ok(Vec::new());
    }
    let k = port_k(if flag == "--udp" { "udp port" } else { "tcp port" });
    value.split(',').map(|p| number(flag, p, &k)).collect()
}

fn word<T>(flag: &str, value: &str, read: fn(&str) -> Option<T>, words: &[&str]) -> Result<T, KnobError> {
    read(value).ok_or_else(|| bad(flag, format!("the value {value:?} is not one of: {}", words.join(", "))))
}

/// The words of `--icmp`: `allow` is sand-net's word for `pass`.
const ICMP_WORDS: [&str; 3] = ["pass", "allow", "drop"];

fn icmp_of(w: &str) -> Option<Icmp> {
    Icmp::from_word(if w == "allow" { "pass" } else { w })
}

impl Knobs {
    /// The knobs of sand-net's flags (`--stack v6`, `--nat both`, `--tcp
    /// 22,443`, `--no-dns`). A flag that is not given keeps its default.
    /// The result is checked (`Knobs::check`).
    pub fn parse_flags<S: AsRef<str>>(args: &[S]) -> Result<Knobs, KnobError> {
        let mut knobs = Knobs::default();
        let mut policy: Option<Policy> = None;
        let mut lists: Option<(Vec<u16>, Vec<u16>)> = None;
        let mut args = args.iter().map(AsRef::as_ref);
        while let Some(flag) = args.next() {
            let switch = match flag {
                "--no-dns" => Some(&mut knobs.dns),
                "--dnat" => Some(&mut knobs.dnat),
                "--hairpin" => Some(&mut knobs.hairpin),
                "--nat64" => Some(&mut knobs.nat64),
                "--dns64" => Some(&mut knobs.dns64),
                "--cgnat" => Some(&mut knobs.cgnat),
                _ => None,
            };
            if let Some(on) = switch {
                *on = flag != "--no-dns";
                continue;
            }
            if !flag.starts_with("--") {
                return Err(bad(flag, "the word is not a flag: each value follows its flag"));
            }
            let value = args.next().ok_or_else(|| bad(flag, "the flag needs a value"))?;
            match flag {
                "--stack" => knobs.stack = word(flag, value, Stack::from_word, Stack::WORDS)?,
                "--nat" => knobs.nat = word(flag, value, Nat::from_word, Nat::WORDS)?,
                "--nat-flags" => knobs.nat_flags = Some(word(flag, value, NatFlags::from_word, NatFlags::WORDS)?),
                "--policy" => policy = Some(word(flag, value, Policy::from_word, Policy::WORDS)?),
                "--tcp" => lists.get_or_insert_with(Default::default).0 = port_list(flag, value)?,
                "--udp" => lists.get_or_insert_with(Default::default).1 = port_list(flag, value)?,
                "--broken" => knobs.broken = Some(word(flag, value, Broken::from_word, Broken::WORDS)?),
                "--mtu" => knobs.mtu = Some(number(flag, value, &MTU_K)?),
                "--icmp" => knobs.icmp = word(flag, value, icmp_of, &ICMP_WORDS)?,
                "--delay" => knobs.delay_ms = Some(number(flag, value.strip_suffix("ms").unwrap_or(value), &DELAY_K)?),
                "--loss" => knobs.loss_bp = Some(percent(flag, value)?),
                "--rate" => knobs.rate_kbit = Some(number(flag, value.strip_suffix("kbit").unwrap_or(value), &RATE_K)?),
                "--ping" => knobs.ping = word(flag, value, Ping::from_word, Ping::WORDS)?,
                _ => return Err(bad(flag, "the flag is not a knob")),
            }
        }
        knobs.policy = match (policy, lists) {
            (Some(_), Some(_)) => return Err(KnobError::Contradiction(Contradiction::PolicyAndPorts)),
            (Some(p), None) => p,
            (None, Some((tcp, udp))) => Policy::Ports { tcp, udp },
            (None, None) => Policy::Open,
        };
        knobs.check()?;
        Ok(knobs)
    }

    /// The flags of this knob set; a default knob gives no flag.
    /// `Knobs::parse_flags` of the result gives the same knobs.
    pub fn flags(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut pair = |flag: &str, value: String| {
            out.push(flag.to_string());
            out.push(value);
        };
        let d = Knobs::default();
        if self.stack != d.stack {
            pair("--stack", self.stack.word().into());
        }
        if self.nat != d.nat {
            pair("--nat", self.nat.word().into());
        }
        if let Some(flags) = self.nat_flags {
            pair("--nat-flags", flags.word().into());
        }
        let join = |list: &[u16]| list.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
        match &self.policy {
            Policy::Open => {}
            // An empty list is not written; a missing list reads as empty.
            // With both empty, `--tcp none` keeps the policy of ports.
            Policy::Ports { tcp, udp } => {
                if !tcp.is_empty() || udp.is_empty() {
                    pair("--tcp", if tcp.is_empty() { NO_PORTS.into() } else { join(tcp) });
                }
                if !udp.is_empty() {
                    pair("--udp", join(udp));
                }
            }
            other => pair("--policy", other.word().unwrap_or("open").into()),
        }
        if let Some(broken) = self.broken {
            pair("--broken", broken.word().into());
        }
        if let Some(mtu) = self.mtu {
            pair("--mtu", mtu.to_string());
        }
        if self.icmp != d.icmp {
            pair("--icmp", self.icmp.word().into());
        }
        if let Some(ms) = self.delay_ms {
            pair("--delay", ms.to_string());
        }
        if let Some(bp) = self.loss_bp {
            pair("--loss", loss_text(bp).trim_end_matches('%').to_string());
        }
        if let Some(kbit) = self.rate_kbit {
            pair("--rate", kbit.to_string());
        }
        if self.ping != d.ping {
            pair("--ping", self.ping.word().into());
        }
        for (on, flag) in [
            (!self.dns, "--no-dns"),
            (self.dnat, "--dnat"),
            (self.hairpin, "--hairpin"),
            (self.nat64, "--nat64"),
            (self.dns64, "--dns64"),
            (self.cgnat, "--cgnat"),
        ] {
            if on {
                out.push(flag.to_string());
            }
        }
        out
    }
}
