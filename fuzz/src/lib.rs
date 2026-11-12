//! The fuzz targets of podcrates (T-017). Each binary of `fuzz_targets/`
//! calls one function of [`targets`], so that the stable test of this crate
//! runs the same checks on the seeds of `seeds/` and on their mutations
//! (`cargo test --manifest-path fuzz/Cargo.toml --lib`). libFuzzer runs them
//! on a nightly Rust (scripts/fuzz.sh).

pub mod targets;

#[cfg(test)]
mod tests;

/// One fuzz target: it takes untrusted bytes. A parse error is a correct
/// answer to bad bytes; a panic, a hang or a large allocation is a defect.
pub type Target = fn(&[u8]);

/// The name and the function of each target. Each name is a binary of
/// `Cargo.toml`, a file of `fuzz_targets/` and a directory of `seeds/`.
pub const TARGETS: &[(&str, Target)] = &[
    ("one_line", targets::one_line),
    ("pem_bundle", targets::pem_bundle),
    ("http_head", targets::http_head),
    ("http_chunked", targets::http_chunked),
    ("socks5_answer", targets::socks5_answer),
    ("proxy_url", targets::proxy_url),
    ("no_proxy", targets::no_proxy),
    ("connect_status", targets::connect_status),
    ("dns_wire", targets::dns_wire),
    ("doh_json", targets::doh_json),
    ("broker_target", targets::broker_target),
    ("gsr_message", targets::gsr_message),
    ("ws_frame", targets::ws_frame),
    ("ws_upgrade", targets::ws_upgrade),
    ("close_reason", targets::close_reason),
    ("quic_probe_answer", targets::quic_probe_answer),
    ("mailbox_message", targets::mailbox_message),
    ("mqtt_packet", targets::mqtt_packet),
    ("session_record", targets::session_record),
    ("session_handover", targets::session_handover),
    ("udp_frame", targets::udp_frame),
    ("e2e_frame", targets::e2e_frame),
    ("e2e_proof", targets::e2e_proof),
    ("e2e_handover", targets::e2e_handover),
    ("hold_sockaddr", targets::hold_sockaddr),
    ("hold_syn", targets::hold_syn),
    ("job_offer", targets::job_offer),
    ("oci_manifest", targets::oci_manifest),
    ("oci_challenge", targets::oci_challenge),
    ("extract_layer", targets::extract_layer),
    ("elf_header", targets::elf_header),
    ("squashfs_image", targets::squashfs_image),
    ("wg_response", targets::wg_response),
    ("wg_cookie", targets::wg_cookie),
    ("wg_transport", targets::wg_transport),
    ("warp_capsule", targets::warp_capsule),
    ("relay_list", targets::relay_list),
    ("relay_pool", targets::relay_pool),
    ("actions_expr", targets::actions_expr),
    ("actions_workflow", targets::actions_workflow),
    ("link_frame", targets::link_frame),
    ("devtunnel_connect_info", targets::devtunnel_connect_info),
    ("netlab_counters", targets::netlab_counters),
];

/// Whether `text` is safe on one line of a terminal, as
/// `pod_core::text::one_line` promises: no control character, no
/// bidirectional formatting character, and no whitespace but single spaces
/// between words, none at the ends.
pub fn is_one_line(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || is_bidi_control(c) || (c.is_whitespace() && c != ' '))
        && !text.starts_with(' ')
        && !text.ends_with(' ')
        && !text.contains("  ")
}

/// Unicode's bidirectional formatting characters, which reorder what a
/// terminal shows. A list of its own, so that the check does not trust the
/// code that it checks.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The input as text, or nothing: the parsers of text take `&str`.
pub fn text(data: &[u8]) -> Option<&str> {
    std::str::from_utf8(data).ok()
}
