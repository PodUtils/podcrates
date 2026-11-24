# pod-tpm

A TPM 2.0 codec in pure Rust, for an ECDSA P-256 key that a TPM makes and
keeps, and that signs only while its policy holds: the PCRs hold the
values of a known boot state (`TPM2_PolicyPCR`), or a policy key signed
the boot state (`TPM2_PolicyAuthorize`). A copy of the key file is of no
use in another TPM. podssh asked for it (PodUtils/podssh#351, COORD-141)
for its key sealed to the machine (PodUtils/podssh@T-839); the node keys
of podnet and podvm can use it too. T-1217 to T-1222 make it (calls C927
and C931).

The base is the module `PodUtils/podssh:crates/podssh-ssh/src/tpm`
(`SOURCES`). The facts of the wire come from the TCG TPM 2.0 Library
specification, parts 1 to 3. `tss-esapi` is not used: it links the C
stack of tpm2-tss. The default build has no C library and no tokio.

## Use

```rust
use pod_tpm::{cmd, public, transact, wire::RH_OWNER};

// A transport is a device, a socket of swtpm, the TBS of Windows, or a
// closure of a test: FnMut(&[u8]) -> Result<Vec<u8>, pod_core::Failure>.
let primary = cmd::Primary::read(&transact(&mut tpm, &cmd::create_primary(RH_OWNER, &public::storage_primary()?)?)?)?;
let template = public::signing_key(&auth_policy)?;
let key = cmd::Created::read(&transact(&mut tpm, &cmd::create(primary.handle, &template)?)?)?;
transact(&mut tpm, &cmd::flush_context(primary.handle)?)?.empty()?;
```

## API

| Item | What |
| --- | --- |
| `Transport` | `submit(&mut self, command) -> Result<Vec<u8>, Failure>`: one whole command out, its whole response back; one command at a time, and it may block. A closure is a transport |
| `transact(transport, command)` | send the command and read the frame of its response: a `Reply` (`handles`, `params`), or an `Error` |
| `wire::{command, response, In, Out, Auth}` | the frame with its bounds (`MAX_COMMAND`, `MAX_RESPONSE`: 4096 bytes, the `TPM_BUFSIZE` of the Linux driver); the reader that checks each size and each count before it reads or allocates; the authorization area (`Auth::EMPTY_PASSWORD`, `Auth::policy`) |
| `rc::{ResponseCode, Code, Subject}` | each response code of part 2 as one line (`TPM_RC_POLICY_FAIL: ..., of session 1 (code 0x099d)`); the format-zero and the format-one codes read apart; `Code::Unknown(u32)` keeps a code that the table does not know; `class()` |
| `cmd::{create_primary, create, load, load_external, sign, verify_signature, flush_context}` | the commands of objects, and the readers `Primary`, `Created`, `Loaded`, `Ticket`, `EcdsaSignature` |
| `cmd::{start_auth_session, policy_pcr, policy_authorize, policy_get_digest}` | the commands of a policy session, and the readers `Started`, `policy_digest` |
| `cmd::{pcr_read, pcr_extend}` | the PCRs, and the reader `PcrValues` |
| `cmd::{get_capability, get_manufacturer}` | the capabilities, and the reader `Capability` (algorithms, handles, properties, the manufacturer) |
| `public::{storage_primary, signing_key, external_p256}` | the template H-2 of the storage primary; the template of a signing key (ECDSA, SHA-256, P-256, `sign`, the `authPolicy` of the caller, no `userWithAuth`); the public area of an outside P-256 key for `TPM2_LoadExternal` |
| `EccPublic::read` | the public area of an ECC P-256 key; another type or curve is refused by its name |
| `PcrSelection` (`parse`, `of`, `add`, `pcrs`, `only`, `count`, `check_policy`, `marshal`, `read`, `from_bytes`, `Display`) | a selection of PCRs in each bank (`TPML_PCR_SELECTION`): at most 16 banks, a bit set of at most 4 octets, PCRs 0 to 23; the text of tpm2-tools, `sha256:0,7+sha1:1` (a bare list is of SHA-256); a bank that was read keeps its bytes |
| `policy::{policy_pcr_digest, policy_pcr_update, pcr_values_digest}` | the digest after `TPM2_PolicyPCR`: `H(old \|\| TPM_CC_PolicyPCR \|\| pcrs \|\| H(values))`, with the hash of the session; a list of values that does not match the selection is refused |
| `policy::{policy_authorize_digest, approved_policy_hash, key_name}` | the digest after `TPM2_PolicyAuthorize` (`H(H(zeros \|\| TPM_CC_PolicyAuthorize \|\| name) \|\| policyRef)`), the hash that the policy key signs (`H(approvedPolicy \|\| policyRef)`), the name of a key (its name algorithm and the hash of its `TPMT_PUBLIC`) |
| `Policy` (`digest`, `approved`), `Statement` (`pcr`, `authorize`, `read_pcr`), `Authorize` | a policy as a key file keeps it: statements of a command code and a body. PolicyPCR, PolicyAuthorize (it sets the digest again from zeros, where it comes), PolicyAuthValue and PolicyPassword; another command is refused by its name |
| `KeyFile` (`loadable`, `to_der`, `to_pem`, `from_der`, `from_pem`, `read`), `AuthPolicy`, `Kind`, `keyfile::is_pem` | the key file `TSS2 PRIVATE KEY` of draft-bottomley-tpm-keys-00, in DER and in PEM (lines of 64): `Kind::Loadable` alone; the policy and the signed policies (`authPolicy`: a name and a `Policy`) as statements, so a file of another program writes back to the same bytes. The old OID of a loadable key (2.23.133.10.2) is read and written as 2.23.133.10.1.3 |
| `Refusal` (`Error::KeyFile`) | why a key file is refused: `Sealed` and `Importable` by name (no `TPM2_Unseal`, no `TPM2_Import`; class `Unsupported`), `UnknownType`, `UnknownField`, `VolatileParent` (MSO 0x80), `BadParent`, `TooLarge`, `TooMany`, `Trailing`, `Malformed` (class `Config`). The bounds: DER of 16 KiB, PEM of 24 KiB, `pubkey` and `privkey` of 4096 bytes and each one whole `TPM2B`, 8 statements and 8 signed policies, a body of 1 KiB, a text of 256 bytes; definite lengths and integers in their fewest octets; each field once and in order; no `secret` in a loadable key; `TPM2_PolicyAuthorize` first in the policy of a key and last in a signed policy (the draft, 4.3 and 5.1.2). The writer refuses the same, so each file that it writes reads back |
| `flow::{primary, session, create_signing, load, trial_digest, flush, nonce, public_point}` | the sequences over a `&mut dyn Transport` and the `&mut dyn Entropy` of the caller: the storage primary H-2 of a hierarchy; a new ECDSA P-256 signing key under the owner's primary or a persistent key such as `0x81000001`, whose `authPolicy` is the digest of a `Policy` that the TPM must reach in a trial session too, as a `KeyFile`; the load of a key file (a persistent parent as it is, a permanent one through its primary; `rsaParent` refused); the point of the public key (65 bytes) |
| `flow::Handle` (`handle`, `transport`, `finish`, `release`), `flow::Held` | the guard of each handle that the TPM gives (a primary, a loaded key, a session, an outside key): it flushes the handle on drop, and a failed flush on drop goes to `tracing`; `finish` flushes and returns the error; `release` keeps a handle that the TPM ended itself (a session after `TPM2_Sign`) |
| `flow::sign::{sign, add_signed_policy, NO_SIGNED_POLICY}` | a signature in a fresh policy session: the PolicyPCR of the key, or each signed policy in turn (its PolicyPCR, `TPM2_LoadExternal` of the policy key in the owner hierarchy, `TPM2_VerifySignature`, `TPM2_PolicyAuthorize` with the ticket, then the rest of the key's policy); `TPM2_PolicyGetDigest` must give the key's `authPolicy` before `TPM2_Sign`; the class `Policy` when the PCRs changed or no signed policy is met ("no signed policy for this boot state"); a new signed policy goes first and replaces one of the same statements |
| `flow::pcr::{pcr_read, policy_pcr_now, pcr_extend, capabilities, Capabilities}` | the values of a selection in its order (8 PCRs a read, again when the counter of updates moved, three times at most); the PolicyPCR statement of the boot state of now; an extend; the manufacturer, the algorithms and `TPM_PT_HR_TRANSIENT_AVAIL` |
| `Error::Step` (`class`, `why`, `cause`) | a step of a sequence that failed, with the error of the command under it (`code()` gives its response code) |
| `HashAlg`, `alg` | the hash algorithms (SHA-1 for the size of a bank alone; SHA-256, SHA-384 and SHA-512 hash) and the algorithm ids |
| `Error` (`class`, `code`) | a response code, a malformed response, a bad value, an algorithm that the crate does not use, a failure of the transport; `Failure::from` at `Stage::Host` |

There is no `Unseal`, no template of a data object and no Ed25519 key
(call C931). A command that needs a nonce takes it as an argument: the
crate reads no random source. It reads no environment variable, keeps no
global, prints nothing and starts no thread.

## The classes of a failure

| Class | When |
| --- | --- |
| `Policy` | the TPM refused by the policy, the PCRs, the authorization or the lockout (`TPM_RC_POLICY_FAIL`, `TPM_RC_PCR_CHANGED`, `TPM_RC_BAD_AUTH`, `TPM_RC_LOCKOUT`, `TPM_RC_INTEGRITY`, ...) |
| `TempFail` | a warning of the TPM (`TPM_RC_RETRY`, `TPM_RC_OBJECT_MEMORY`, ...) |
| `Unsupported` | the TPM does not have the algorithm, the curve or the command; a code of a TPM 1.2; a key of another type |
| `Unavailable` | the TPM does not run (`TPM_RC_FAILURE`, `TPM_RC_INITIALIZE`), a layer in front of it refused, or the transport found no TPM |
| `Usage` | a value of the caller cannot go into a command; a handle that is not loaded |
| `Protocol` | a response that fails a bound; another refusal of the TPM |

## Features

None in this version. `device` and `tbs` (the transports, T-1221),
`soft` (a TPM in software) and `fuzzing` (T-1222) come with their code.

## Adoption map (podssh)

| podssh (`crates/podssh-ssh/src/tpm/`) | pod-tpm |
| --- | --- |
| `wire.rs`: `Out`, `In`, `command`, `response`, `Fault` | `wire`, `Error` (`Fault::Tpm` is `Error::Tpm`; `Fault::Malformed` is `Error::Malformed`); `MAX_MESSAGE` (8192) is `MAX_COMMAND` and `MAX_RESPONSE` (4096) |
| `wire::Session`, `EMPTY_PASSWORD` | `wire::Auth`, `Auth::EMPTY_PASSWORD`, `Auth::policy` |
| `rc.rs`: `describe`, `base`, `format_one`, the constants | `ResponseCode` (`Display`, `code`, `subject`, `class`) and `Code` |
| `cmd.rs` | `cmd`; the readers return types (`Created`, `PcrValues`, `Capability`); `create_primary` takes the hierarchy; `sign_by_password` is `sign` with `Auth::EMPTY_PASSWORD` |
| `authorize.rs`: `load_external`, `verify_signature`, `ticket`, `null_ticket`, `policy_authorize`, `signer_public` | `cmd::load_external` (the hierarchy is an argument), `cmd::verify_signature`, `Ticket::read`, `Ticket::null`, `cmd::policy_authorize`, `public::external_p256` |
| `public.rs`: `storage_template`, `sealed_template`, `EccPublic::parse`, `signature_blob` | `public::storage_primary`, `public::signing_key`, `EccPublic::read` (the point of 65 bytes), `EcdsaSignature::read`; podssh keeps `EccPublic::public_key` and the SSH blob of a signature |
| `policy.rs`: `Pcrs` (the SHA-256 bank alone), `Pcrs::parse`, `text`, `selection`, `read_selection` | `PcrSelection` (each bank), `PcrSelection::parse`, `Display` (`sha256:0,7,11`), `marshal`, `read` with `only(HashAlg::Sha256)` |
| `policy.rs`: `pcr_digest`, `pcr_body`, `split_pcr_body`, `digest`, `sha256` | `policy::pcr_values_digest`, `Statement::pcr`, `Statement::read_pcr`, `Policy::digest`, `policy::hash` |
| `authorize.rs`: `Authorize`, `name`, `digest_after`, `signed` | `policy::Authorize`, `policy::key_name` (SHA-256, SHA-384, SHA-512), `policy::policy_authorize_digest`; the signature of `signed` stays in podssh with its `PolicyKey`, over `policy::approved_policy_hash` |
| `flow::Transport::transact(&[u8]) -> Result<Vec<u8>, String>` | `Transport::submit(&[u8]) -> Result<Vec<u8>, Failure>` |
| `keyfile.rs`: `KeyFile` (`policy: Vec<(u32, Vec<u8>)>`, `auth_policies`), `AuthPolicy`, `to_der`, `to_pem`, `from_der`, `from_pem`, `is_pem`, `MAX_FILE` (64 KiB) | `keyfile::KeyFile` (`kind`, `policy: Policy`, `auth_policy`), `AuthPolicy` (`policy: Policy`), the same functions, which return `Error`; `to_der` and `to_pem` check the bounds first; `MAX_DER` (16 KiB) and `MAX_PEM` (24 KiB). A sealed key, an importable key and a volatile parent are refused by name; an unknown optional field is refused, not passed over |
| `flow.rs`: `Transport::transact`, `run`, `flush`, `manufacturer`, `read_pcrs`, `make`, `create_sealed`, `trial_digest`, `start_session`, `load`, `sign`, `sign_in`, `run_step`, `Unmet`, `refused` | `Transport::submit`, `transact`, `flow::flush` and the guard `flow::Handle`, `pcr::capabilities`, `pcr::pcr_read` (each bank), `pcr::policy_pcr_now` with `create_signing` (the parent is an argument: `0x40000001` or `0x81000001`), `flow::trial_digest` (a whole `Policy`), `flow::session`, `flow::load`, `sign::sign` (a loaded key in, `EcdsaSignature` out), `Error::Step` with the class `Policy` |
| `flow_signed.rs`: `make`, `add_policy`, `head`, `sign`, `attempt`, `take_signed`, `summary` | `create_signing` of a `Policy` of one `TPM2_PolicyAuthorize`, then `sign::add_signed_policy` of the caller's signed policy (podssh's `PolicyKey` signs `policy::approved_policy_hash`); `sign::sign` |
| `flow_tests.rs`, `signed_tests.rs` | `tests/flow.rs` with the fake TPM `tests/common/fake.rs`; the tests that need `soft.rs` or podssh's keygen wait for T-1222 |
| `codec_tests.rs` | `tests/frame.rs`, `tests/commands.rs`, `tests/policy.rs`; its tests of the key file in `tests/keyfile.rs` |
| `testdata/p256-authvalue.b64` | `tests/testdata/p256-authvalue.b64` |

podssh keeps `mod.rs` (the probe, `PODSSH_TPM_DEVICE`, the SSH forms),
`signer.rs` and its `PolicyKey` (p256 signs off the TPM). The patch
adoption/podssh/pod-tpm.patch comes with the last entry of the crate.

## Tests

`cargo test -p pod-tpm`. `tests/commands.rs`: each command against the
bytes of part 3, written by hand (podssh's vectors, and those of
`Create`, `Load`, `PCR_Extend`, `StartAuthSession`, `PolicyPCR` and `Sign`);
the readers of each response; the signature as `r` and `s`; a public
area of another type or curve; the bad values of a caller.
`tests/frame.rs`: a response read or refused with its reason; a response
over `MAX_RESPONSE`; each cut and a byte more; a sized buffer and a count
past their bound; each changed byte of a response, never a panic; each
code with its name, its place and its class. `tests/policy.rs`: the
selection as a user names it and as the TPM reads it; each PCR alone;
the bounds of a selection; the digests of PolicyPCR (podssh's vector, and
by hand for a session of SHA-384, two banks and a second PolicyPCR), of
PolicyAuthorize, of the approved policy and of a name; the values that
do not match; the order of a policy; each bit of each input changes the
digest. `tests/keyfile.rs`: the files of go-tpm-keyfiles (a policy of
PCRs and PolicyAuthValue; a signed policy) read and written back to the
same bytes; the layout of a signed policy written by hand from the draft;
a key under a policy of PCRs and under signed policies in DER and PEM; the
old OID. `tests/flow.rs`, against a fake TPM: a key made and signing,
refused after the boot state changed, refused in another TPM; a
persistent parent; a key under a policy key that signs again after a
signed policy of the new state, and a policy of another key or a changed
signature refused; each command of each sequence failed in turn, refused,
lost or answered with its handles alone, and no handle stays.
`tests/keyfile_bounds.rs`: each refusal by its name; each cut and
each bit flip of a good file refused or read back to the same bytes. The
digest that swtpm gives is the proof of T-1222. The plants
`tpm-response-over-the-frame-bound`, `tpm-keyfile-trailing-bytes` and
`tpm-flow-external-key-not-flushed` take away the bound of the frame, the
check of the bytes after a key file and the guard of the policy key.
