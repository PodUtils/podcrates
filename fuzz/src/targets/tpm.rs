//! The readers of `pod-tpm` (T-1339): the response of a TPM and the key
//! file `TSS2 PRIVATE KEY`, each untrusted input.

use pod_tpm::cmd::pcr::MAX_VALUES;
use pod_tpm::cmd::{self, Capability, CapabilityData, Created, Loaded, PcrValues, Primary, Started, Ticket};
use pod_tpm::keyfile::{self, MAX_AUTH_POLICIES, MAX_BODY, MAX_DER, MAX_PART, MAX_PEM, MAX_STATEMENTS, MAX_TEXT};
use pod_tpm::selection::MAX_BANKS;
use pod_tpm::wire::{HEADER, In, MAX_DIGEST, MAX_NAME, MAX_RESPONSE};
use pod_tpm::{Authorize, EccPublic, EcdsaSignature, Error, HashAlg, KeyFile, PcrSelection, Policy, Reply};

use crate::{is_one_line, text};

/// The fourteen commands of the crate. The first byte of an input of
/// `tpm_response` picks one: its code tells how many handles the response
/// holds.
pub const COMMANDS: [u32; 14] = [
    cmd::CC_CREATE_PRIMARY,
    cmd::CC_CREATE,
    cmd::CC_LOAD,
    cmd::CC_LOAD_EXTERNAL,
    cmd::CC_SIGN,
    cmd::CC_VERIFY_SIGNATURE,
    cmd::CC_FLUSH_CONTEXT,
    cmd::CC_START_AUTH_SESSION,
    cmd::CC_POLICY_PCR,
    cmd::CC_POLICY_AUTHORIZE,
    cmd::CC_POLICY_GET_DIGEST,
    cmd::CC_PCR_READ,
    cmd::CC_PCR_EXTEND,
    cmd::CC_GET_CAPABILITY,
];

/// The response of a TPM to one of the fourteen commands (`Reply::read`,
/// then each reader of `cmd` on the parameters; new in `pod-tpm`, T-1217):
/// a frame that reads has 10 to `MAX_RESPONSE` bytes and the handles of its
/// command; a refusal of the TPM is one line; each value that a reader
/// gives is in the bound of part 2, and a selection, a signature and a
/// policy body write back to the bytes that came.
pub fn tpm_response(data: &[u8]) {
    let Some((&pick, bytes)) = data.split_first() else { return };
    let code = COMMANDS[usize::from(pick) % COMMANDS.len()];
    let reply = match Reply::read(code, bytes) {
        Ok(reply) => reply,
        Err(Error::Tpm(rc)) => {
            assert!((HEADER..=MAX_RESPONSE).contains(&bytes.len()));
            let _ = rc.class();
            assert!(is_one_line(&rc.to_string()), "{rc}");
            return;
        }
        Err(why) => {
            let _ = why.class();
            assert!(!why.to_string().is_empty());
            return;
        }
    };
    assert!((HEADER..=MAX_RESPONSE).contains(&bytes.len()));
    assert_eq!(reply.handles.len(), cmd::response_handles(code));
    assert!(HEADER + 4 * reply.handles.len() + reply.params.len() <= bytes.len());
    let size = reply.params.len();
    if let Ok(p) = Primary::read(&reply) {
        assert!(p.name.len() <= MAX_NAME && p.public.len() <= size);
        public_area(&p.public);
    }
    if let Ok(c) = Created::read(&reply) {
        assert!(c.private.len() + c.public.len() <= size);
        assert!(whole(&c.private) && whole(&c.public));
        public_area(&c.public);
    }
    if let Ok(l) = Loaded::read(&reply) {
        assert!(l.name.len() <= MAX_NAME);
    }
    if let Ok(signature) = EcdsaSignature::read(&reply) {
        let again = EcdsaSignature::read_from(&mut In::new(&signature.to_tpmt())).expect("a written signature reads");
        assert_eq!(again, signature);
    }
    if let Ok(Ticket(ticket)) = Ticket::read(&reply) {
        assert!(ticket.len() <= 8 + MAX_DIGEST && reply.params.starts_with(&ticket));
    }
    if let Ok(s) = Started::read(&reply) {
        assert!(s.nonce_tpm.len() <= MAX_DIGEST);
    }
    if let Ok(digest) = cmd::policy_digest(&reply) {
        assert!(digest.len() <= MAX_DIGEST);
    }
    if let Ok(v) = PcrValues::read(&reply) {
        assert!(v.values.len() <= MAX_VALUES && v.values.len() == v.selection.count());
        assert!(v.values.iter().all(|value| value.len() <= MAX_DIGEST));
        selection(&v.selection);
    }
    if let Ok(c) = Capability::read(&reply) {
        let held = match &c.data {
            CapabilityData::Algorithms(list) => 6 * list.len(),
            CapabilityData::Handles(list) => 4 * list.len(),
            CapabilityData::Properties(list) => 8 * list.len(),
            CapabilityData::Other(_, rest) => rest.len(),
        };
        assert!(held <= size);
        let _ = c.manufacturer();
    }
    let _ = reply.empty();
}

/// A key file from untrusted bytes (`KeyFile::read`, and `from_pem` of the
/// text; new in `pod-tpm`, T-1219): each size and each count of a file that
/// reads is in its bound; its DER reads to the same file and writes the
/// same DER again, and so does its PEM text; the DER of the input is the
/// DER that the writer gives, except for the forms that the draft lets a
/// reader take.
pub fn tpm_keyfile(data: &[u8]) {
    if let Some(input) = text(data)
        && let Ok(file) = KeyFile::from_pem(input)
    {
        check_keyfile(&file);
    }
    let Ok(file) = KeyFile::read(data) else { return };
    check_keyfile(&file);
    if !keyfile::is_pem(data) && !lenient(data) {
        assert_eq!(file.to_der().expect("a file that reads writes"), data);
    }
}

fn check_keyfile(file: &KeyFile) {
    assert!(file.public.len() <= MAX_PART && file.private.len() <= MAX_PART);
    assert!(file.description.as_ref().is_none_or(|d| d.len() <= MAX_TEXT));
    assert!(file.auth_policy.len() <= MAX_AUTH_POLICIES);
    policy(&file.policy);
    for signed in &file.auth_policy {
        assert!(signed.name.as_ref().is_none_or(|n| n.len() <= MAX_TEXT));
        policy(&signed.policy);
    }
    let der = file.to_der().expect("a file that reads writes");
    assert!(der.len() <= MAX_DER);
    let again = KeyFile::from_der(&der).expect("a written file reads");
    assert_eq!(&again, file);
    assert_eq!(again.to_der().expect("a file that reads writes"), der);
    let pem = file.to_pem().expect("a file that reads writes");
    assert!(pem.len() <= MAX_PEM);
    assert_eq!(&KeyFile::read(pem.as_bytes()).expect("a written PEM text reads"), file);
    public_area(&file.public);
}

/// The statements of a policy: in their bounds, a `TPM2_PolicyAuthorize`
/// writes back to its body, a `TPM2_PolicyPCR` gives a selection in its
/// bounds. The digest of the policy does not panic.
fn policy(policy: &Policy) {
    assert!(policy.statements.len() <= MAX_STATEMENTS);
    for statement in &policy.statements {
        assert!(statement.body.len() <= MAX_BODY);
        match statement.code {
            cmd::CC_POLICY_AUTHORIZE => {
                if let Ok(authorize) = Authorize::parse(&statement.body) {
                    assert_eq!(authorize.to_bytes().expect("a read body writes"), statement.body);
                }
            }
            cmd::CC_POLICY_PCR => {
                if let Ok((pcrs, digest)) = statement.read_pcr() {
                    assert!(digest.len() <= MAX_DIGEST);
                    selection(&pcrs);
                }
            }
            _ => {}
        }
    }
    let _ = (policy.digest(HashAlg::Sha256), policy.approved(HashAlg::Sha256));
}

/// A selection that was read: at most 16 banks, and its bytes read back to
/// the same selection.
fn selection(pcrs: &PcrSelection) {
    assert!(pcrs.banks().len() <= MAX_BANKS);
    assert_eq!(&PcrSelection::from_bytes(&pcrs.marshal()).expect("a read selection reads again"), pcrs);
    assert!(is_one_line(&pcrs.to_string()));
}

/// A public area: an authPolicy of a digest at most, an uncompressed point.
fn public_area(whole: &[u8]) {
    if let Ok(key) = EccPublic::read(whole) {
        assert!(key.auth_policy.len() <= MAX_DIGEST);
        assert_eq!(key.point[0], 4);
    }
}

/// Whether `bytes` are one whole `TPM2B`.
fn whole(bytes: &[u8]) -> bool {
    let mut input = In::new(bytes);
    input.sized().is_ok() && input.is_empty()
}

/// Whether the DER holds a form that the reader takes and the writer does
/// not give: the OID of a loadable key of the old files of
/// openssl_tpm2_engine, or an explicit BOOLEAN (`emptyAuth`, `rsaParent`)
/// of an octet other than 0xFF (the draft, section 6), which also holds
/// false, which the writer leaves out.
fn lenient(der: &[u8]) -> bool {
    const OLD_OID: &[u8] = &[0x06, 0x05, 0x67, 0x81, 0x05, 0x0A, 0x02];
    der.windows(OLD_OID.len()).any(|w| w == OLD_OID)
        || der.windows(5).any(|w| matches!(w, [0xA0 | 0xA5, 0x03, 0x01, 0x01, octet] if *octet != 0xFF))
}
