//! The map of the codes of the TBS of Windows to a failure: it runs on each
//! host. The test of the TBS of this host runs on Windows alone, and skips
//! with its reason when the host has no TPM.
#![cfg(feature = "tbs")]

use pod_core::{Class, Stage};
use pod_tpm::transport::tbs;

#[test]
fn each_tbs_code_has_the_class_of_its_meaning() {
    let cases = [
        (tbs::TBS_E_TPM_NOT_FOUND, Class::Unavailable),
        (tbs::TBS_E_SERVICE_NOT_RUNNING, Class::Unavailable),
        (tbs::TBS_E_SERVICE_DISABLED, Class::Unavailable),
        (tbs::TBS_E_ACCESS_DENIED, Class::Permission),
        (tbs::TBS_E_SERVICE_START_PENDING, Class::TempFail),
        (tbs::TBS_E_TOO_MANY_TBS_CONTEXTS, Class::TempFail),
        (tbs::TBS_E_TOO_MANY_RESOURCES, Class::TempFail),
        (tbs::TBS_E_COMMAND_CANCELED, Class::TempFail),
        (tbs::TBS_E_INSUFFICIENT_BUFFER, Class::Protocol),
        (tbs::TBS_E_BUFFER_TOO_LARGE, Class::Usage),
        (tbs::TBS_E_BAD_PARAMETER, Class::Usage),
        (tbs::TBS_E_INVALID_CONTEXT, Class::Usage),
        (tbs::TBS_E_INVALID_CONTEXT_PARAM, Class::Unsupported),
        (tbs::TBS_E_IOERROR, Class::Unavailable),
        (tbs::TBS_E_INTERNAL_ERROR, Class::Internal),
        (0x8028_4099, Class::Unavailable),
    ];
    for (code, class) in cases {
        let failure = tbs::failure("Tbsip_Submit_Command", code);
        assert_eq!(failure.class, class, "{code:#x}: {failure}");
        assert_eq!(failure.stage, Stage::Host, "{code:#x}");
    }
}

#[test]
fn a_tbs_code_says_its_call_its_name_and_its_number() {
    let text = tbs::failure("Tbsi_Context_Create", tbs::TBS_E_TPM_NOT_FOUND).to_string();
    assert!(text.contains("Tbsi_Context_Create"), "{text}");
    assert!(text.contains("TBS_E_TPM_NOT_FOUND"), "{text}");
    assert!(text.contains("0x8028400f"), "{text}");
    assert!(text.contains("no TPM"), "{text}");
}

#[test]
fn an_unknown_tbs_code_keeps_its_number() {
    assert_eq!(tbs::name(0x8028_4099), None);
    let text = tbs::failure("Tbsip_Submit_Command", 0x8028_4099).to_string();
    assert!(text.contains("0x80284099"), "{text}");
}

#[test]
fn each_known_tbs_code_has_a_name() {
    for code in 0x8028_4001..=0x8028_4016u32 {
        let name = tbs::name(code).unwrap_or_else(|| panic!("{code:#x} has no name"));
        assert!(name.starts_with("TBS_E_"), "{code:#x}: {name}");
    }
}

#[cfg(windows)]
#[test]
fn the_tbs_of_this_host_answers() {
    let mut tpm = match tbs::Tbs::open() {
        Ok(tpm) => tpm,
        Err(e) if e.class == Class::Unavailable => {
            pod_testkit::host::skip_line(&format!("this host has no TPM: {e}"));
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let caps = pod_tpm::flow::pcr::capabilities(&mut tpm).expect("the capabilities");
    assert!(caps.manufacturer.is_some(), "no manufacturer");
}
