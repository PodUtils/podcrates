//! The values that the consumer reads from its environment and its settings
//! file, once, and gives the crate (`docs/design.md`, section 1, rule 6).
//! The crate reads no variable. Each field holds the text of a variable as
//! the consumer read it, so that the crate reads each word as the consumer's
//! own reader did; [`Names`] holds the names that the words of the crate
//! cite.

use pod_identity::KeyName;

use crate::pool::members::Member;

/// What the consumer gives.
#[derive(Debug, Clone)]
pub struct Settings {
    /// A relay token that the user gave: taken before the cache and a mint
    /// (podssh: `PODSSH_RELAY_TOKEN`).
    pub token: Option<String>,
    /// Connect to no relay (podssh: `PODSSH_OFFLINE`).
    pub offline: bool,
    /// The keys that a signed list of relay hosts may be signed by, as text
    /// (podssh: `PODSSH_RELAY_KEYS`); `none` trusts no key.
    pub keys: Option<String>,
    /// More keys, each with where it is trusted from: the consumer's
    /// settings file and the relay members of its net.
    pub keys_by_settings: Vec<(KeyName, String)>,
    /// The relay members of the net that this host holds, asked for their
    /// lists of hosts.
    pub members: Vec<Member>,
    /// The word of the rank (podssh: `PODSSH_RELAY_RANK`): `no` leaves the
    /// rank out of the order, `auto` ranks in the background.
    pub rank: Option<String>,
    /// The settings file's word of the rank, below the variable: `false`
    /// leaves it out (podssh: `RelayRank no`).
    pub rank_by_file: bool,
    /// The settings file asks for a rank in the background (podssh:
    /// `RelayRank auto`).
    pub rank_auto_by_file: bool,
    /// For tests only, `DELAY_MS,GAP_MS` of the background rank; read by a
    /// build with debug assertions alone (podssh: `PODSSH_RELAY_RANK_PACE`).
    pub rank_pace: Option<String>,
    /// Which sessions take HTTP/2 (podssh: `PODSSH_H2`): `auto`, `yes`, `no`.
    pub h2: Option<String>,
    /// Which sessions race QUIC (podssh: `PODSSH_QUIC`): `auto`, `yes`, `no`.
    pub quic: Option<String>,
    /// Whether the cleartext road comes last (podssh: `PODSSH_CLEARTEXT`).
    pub cleartext: Option<String>,
    /// The names that the words of the crate cite.
    pub names: Names,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            token: None,
            offline: false,
            keys: None,
            keys_by_settings: Vec::new(),
            members: Vec::new(),
            rank: None,
            rank_by_file: true,
            rank_auto_by_file: false,
            rank_pace: None,
            h2: None,
            quic: None,
            cleartext: None,
            names: Names::default(),
        }
    }
}

impl Settings {
    /// Whether the user gave a token.
    pub fn token_given(&self) -> bool {
        self.token.as_deref().is_some_and(|v| !v.trim().is_empty())
    }
}

/// The names of the consumer that the crate's words cite: its name, and the
/// variable or the flag of each setting. The default names are neutral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    /// The program: `podssh`, `podnet`.
    pub app: String,
    pub token: String,
    pub offline: String,
    pub keys: String,
    pub rank: String,
    pub h2: String,
    pub quic: String,
    pub cleartext: String,
    pub failover: String,
    pub cache_dir: String,
}

impl Default for Names {
    fn default() -> Names {
        Names {
            app: "this client".into(),
            token: "the relay token setting".into(),
            offline: "the offline setting".into(),
            keys: "the setting of the relay keys".into(),
            rank: "the rank setting".into(),
            h2: "the HTTP/2 setting".into(),
            quic: "the QUIC setting".into(),
            cleartext: "the cleartext setting".into(),
            failover: "the failover setting".into(),
            cache_dir: "the cache directory setting".into(),
        }
    }
}

impl Names {
    /// podssh's names: its variables.
    pub fn podssh() -> Names {
        Names {
            app: "podssh".into(),
            token: "PODSSH_RELAY_TOKEN".into(),
            offline: "PODSSH_OFFLINE".into(),
            keys: "PODSSH_RELAY_KEYS".into(),
            rank: "PODSSH_RELAY_RANK".into(),
            h2: "PODSSH_H2".into(),
            quic: "PODSSH_QUIC".into(),
            cleartext: "PODSSH_CLEARTEXT".into(),
            failover: "PODSSH_RELAY_FAILOVER".into(),
            cache_dir: "PODSSH_CACHE_DIR".into(),
        }
    }
}
