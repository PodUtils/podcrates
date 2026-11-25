//! A stream that ended for good, with no relay and no HTTP in the cause
//! (PodUtils/podssh#359): its own words alone, final, exit code 69.

use pod_core::{Class, Failure, Stage};

use super::ConnectError;

const WORDS: &str = "the carrier dns:FILE ended, and a stream that this end did not start has one life";

/// The line is the words of the caller, with no word of a relay or of HTTP.
#[test]
fn an_ended_stream_is_shown_in_its_own_words() {
    let line = ConnectError::Ended(WORDS.into()).to_string();
    assert_eq!(line, WORDS);
    for word in ["relay", "HTTP", "410", "refused"] {
        assert!(!line.contains(word), "{word}: {line}");
    }
}

/// The stage is the session; the class keeps the exit code 69; no I/O cut it.
#[test]
fn an_ended_stream_is_out_of_reach_with_exit_code_69() {
    let ended = ConnectError::Ended(WORDS.into());
    assert_eq!(ended.stage_and_class(), (Stage::Session, Class::Unavailable));
    assert_eq!(ended.stage_and_class().1.exit_code(), 69);
    assert_eq!(ended.cut(), None);
    let failure = Failure::from(ended);
    assert_eq!((failure.stage, failure.class), (Stage::Session, Class::Unavailable));
    assert!(failure.to_string().contains(WORDS), "{failure}");
    assert!(!failure.to_string().contains("relay"), "{failure}");
}
