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
// With the feature `device`:
// let mut tpm = pod_tpm::transport::Device::open(Path::new("/dev/tpmrm0"), Limits::default())?;
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
| `transport::Device` (`open`, `from_fd`; feature `device`, Unix) | the device of a Unix kernel by the path of the caller (`/dev/tpmrm0`; `/dev/tpm0` has no resource manager, so the caller opts in by its path): one write of the command, one read of at most 4096 bytes whose size field must equal the bytes that came; not blocking, with poll(2) to the deadline. A socket, a regular file or a directory is refused before the open; a FIFO is the stand-in of a test. `from_fd` takes a descriptor that a helper opened |
| `transport::Socket` (`connect_tcp`, `connect_unix`, `from_tcp`, `from_unix`, `with_framing`; feature `device`) | the command channel of swtpm (`--server type=tcp` or `type=unixio`) or of the simulator of the reference; `Framing::Raw` (swtpm's own: the bytes as they are) or `Framing::Mssim` (the prefix `TPM_SEND_COMMAND`, locality 0 and the size; the size of the response, the response, an acknowledgement of 0). The size of a response is checked (10 to 4096) before a byte more is read. The control channel and `TPM2_Startup` are the harness's |
| `transport::Limits` (`command`) | the time limit of one command, from the write to the last byte (default 30 s, podssh's); past it the transport closes its file or its connection and is broken |
| `transport::tbs::Tbs` (`open`; feature `tbs`, Windows) | a TBS context of a TPM 2.0 (`Tbsi_Context_Create`, `TBS_CONTEXT_PARAMS2` with `includeTpm20`), `Tbsip_Submit_Command` at locality 0 and the normal priority into a buffer of 4096 bytes, `Tbsip_Context_Close` on drop; the call blocks until the TPM answers |
| `soft::SoftTpm` (`new`, `extend`, `persist`, `pcr`, `loaded`, `commands`; feature `soft`) | a model of a TPM, not a TPM, as a `Transport`: the fourteen commands with each field of each response and of each session, one PCR bank (SHA-256), keys of ECDSA P-256 from its seed (the same seed is the same TPM), a private part that loads only under its parent in a TPM of the same seed (`TPM_RC_INTEGRITY`), the policies of PCRs and of a policy key with the checks of part 3 (`TPM_RC_POLICY_FAIL`, `TPM_RC_VALUE`, `TPM_RC_SIGNATURE`, `TPM_RC_AUTH_UNAVAILABLE`), three transient objects and three sessions (`TPM_RC_OBJECT_MEMORY`); the code of a TPM for each refusal. Its digests of a policy come from the bytes of the command, not from `policy` |
| `transport::tbs::{failure, name, TBS_E_*}` (feature `tbs`, each system) | each code of TBS with its name, its words and its class (`Unavailable` for no TPM, a service that does not run and a code that the table does not know; `Permission` for `TBS_E_ACCESS_DENIED`; `TempFail` for a service that starts or too many contexts); the Windows build checks each code against `windows-sys` |

There is no `Unseal`, no template of a data object and no Ed25519 key
(call C931). A command that needs a nonce takes it as an argument: the
crate reads no random source. It reads no environment variable, keeps no
global, prints nothing and starts no thread.

## The classes of a failure

| Class | When |
| --- | --- |
| `Timeout` | a transport did not get the whole response in `Limits::command`: it closes its device or connection |
| `Permission` | the system refused the open of the device, or TBS refused the command (`TBS_E_ACCESS_DENIED`) |
| `Policy` | the TPM refused by the policy, the PCRs, the authorization or the lockout (`TPM_RC_POLICY_FAIL`, `TPM_RC_PCR_CHANGED`, `TPM_RC_BAD_AUTH`, `TPM_RC_LOCKOUT`, `TPM_RC_INTEGRITY`, ...) |
| `TempFail` | a warning of the TPM (`TPM_RC_RETRY`, `TPM_RC_OBJECT_MEMORY`, ...) |
| `Unsupported` | the TPM does not have the algorithm, the curve or the command; a code of a TPM 1.2; a key of another type |
| `Unavailable` | the TPM does not run (`TPM_RC_FAILURE`, `TPM_RC_INITIALIZE`), a layer in front of it refused, or the transport found no TPM (no device at the path, no listener, a closed connection, TBS with no TPM 2.0); each call of a transport that broke on an earlier command |
| `Usage` | a value of the caller cannot go into a command; a handle that is not loaded; a command of fewer than 10 or more than 4096 bytes given to a transport |
| `Protocol` | a response that fails a bound (a transport reads at most 4096 bytes); a short response; an acknowledgement of the `mssim` interface that is not 0; another refusal of the TPM |

## Features

| Feature | What |
| --- | --- |
| `device` | `transport::Device` (Unix, with poll(2) of `libc`) and `transport::Socket` (std alone) |
| `tbs` | `transport::tbs::Tbs` on Windows (`windows-sys`: raw-dylib imports of `tbs.dll`, a DLL of the system); the map of the codes of TBS on each system |
| `soft` | `soft::SoftTpm`, a TPM in software for the tests of this crate and of a consumer (`p256`, pure Rust): a consumer turns it on in `[dev-dependencies]` alone |

The default build is the codec alone.

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
| `flow_tests.rs`, `signed_tests.rs` | `tests/flow.rs` with the fake TPM `tests/common/fake.rs`; `a_signer_that_ignores_the_policy_is_refused` and `a_ticket_of_nothing_is_refused_outside_a_trial` in `tests/soft.rs`, `tests/swtpm.rs` and `tests/mssim.rs` (`tests/common/proof.rs`) |
| `soft.rs`: `SoftTpm` (`new`, `extend`, `loaded`, `messages`, `sent`) | `soft::SoftTpm` (feature `soft`): `new` takes a seed, `extend`, `loaded`, `commands`, `persist`, `pcr`; it signs the digest itself (`p256`, no list of messages, no aws-lc-rs) and gives each field of each response |
| `unix.rs`: `open`, `Device` (`transact`), `Socket` (`transact`), `wait`, `whole`; `COMMAND_LIMIT` of `mod.rs` | `transport::Device::open` and `transport::Socket::connect_unix` (the caller chooses the one of its path); one deadline for each command (`Limits::command`), not a time limit for each read; a response of at most 4096 bytes whose size field equals the bytes of the read (podssh read 8192 and did not check the size field); after a failure the transport is closed |
| `unix.rs`: `Place`, `find`, `find_in` | stay in podssh: the probe of `/dev/tpmrm0` and `/dev/tpm0` with `PODSSH_TPM_DEVICE` (the crate reads no variable and chooses no path) |
| `other.rs` | `transport::tbs::Tbs`: the TBS of Windows in the place of the refusal |
| `codec_tests.rs` | `tests/frame.rs`, `tests/commands.rs`, `tests/policy.rs`; its tests of the key file in `tests/keyfile.rs` |
| `testdata/p256-authvalue.b64` | `tests/testdata/p256-authvalue.b64` |

podssh keeps `mod.rs` (the probe, `PODSSH_TPM_DEVICE`, the SSH forms),
`signer.rs` and its `PolicyKey` (p256 signs off the TPM). podssh took the
crate at `806e0a4`.

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
each bit flip of a good file refused or read back to the same bytes.
`tests/socket.rs` (feature `device`), against the stand-in
`tests/standin/` that reads each command in one read as swtpm does: both
framings, a response of 4096 bytes, a response over the bound refused
before it is read, a short response, a bad acknowledgement, a closed
socket, a late response (the socket closes), a command out of its bounds
refused before it is sent, a Unix socket of a path and the TCP channel.
`tests/device.rs` (feature `device`, Unix), with a socket pair or a FIFO
in the place of the device: one write and one read, the bound, a short
and a late response, the close on drop, a missing, a refused and a wrong
file; the device of this host when it has one (else a skip with its
reason). `tests/tbs.rs` (feature `tbs`): the class, the name and the
number of each code of TBS on each host; the TBS of this host on
Windows. The proof against a TPM, `tests/common/proof.rs`, runs against
three TPMs: `tests/soft.rs` (feature `soft`), `tests/swtpm.rs` (swtpm on
a Unix socket of the test, `Framing::Raw`) and `tests/mssim.rs` (the
simulator of `ms-tpm-20-ref` on 127.0.0.1, `Framing::Mssim`): a key
sealed to PCR 7 signs and `p256` verifies the signature, and after
`TPM2_PCR_Extend` it is refused; a key under a policy key signs again
after a signed policy of the new state; a copy does not load in a
second TPM; the password session and the ticket of nothing are refused
by the TPM; the digests of `policy` are the TPM's (a trial session); no
handle stays (`TPM2_GetCapability` of the handles); the time of
`TPM2_CreatePrimary`. Each test of swtpm and of the simulator skips with
its reason on a host with none (`tests/sim/mod.rs`);
`scripts/ci/tpm-tests.sh` runs them in `ci.yml` and fails on a skip.
`tests/soft.rs` also holds the model: each field of a response, the same
seed as the same TPM, a persistent parent, the refusals of a TPM. The plants
`tpm-response-over-the-frame-bound`, `tpm-keyfile-trailing-bytes`,
`tpm-flow-external-key-not-flushed` and
`tpm-socket-response-over-the-bound` take away the bound of the frame, the
check of the bytes after a key file, the guard of the policy key and the
bound of the read of the socket transport; `tpm-soft-primary-without-its-name`
takes the name out of the response of `TPM2_CreatePrimary` of the TPM in
software. The plants of the bounds of the readers take away one bound
each: the size field of a frame, `In::sized_max`, the two checks of
`In::count`, 16 banks and 4 octets of a selection, 8 values of
`TPM2_PCR_Read`, the size of an item of a list of `TPM2_GetCapability`, 32
bytes of a coordinate of P-256, and `MAX_DER`, `MAX_PEM`, `MAX_PART`,
`MAX_STATEMENTS`, `MAX_AUTH_POLICIES`, `MAX_BODY` and `MAX_TEXT` of a key
file (`scripts/plants/tpm-*.plant`). The fuzz targets `tpm_response` and
`tpm_keyfile` of `fuzz/` run the readers of a response and of a key file.
