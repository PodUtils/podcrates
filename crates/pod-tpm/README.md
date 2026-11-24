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
| `HashAlg`, `alg` | the hash algorithms and the algorithm ids |
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
| `flow::Transport::transact(&[u8]) -> Result<Vec<u8>, String>` | `Transport::submit(&[u8]) -> Result<Vec<u8>, Failure>` |
| `codec_tests.rs` | `tests/frame.rs`, `tests/commands.rs` |

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
code with its name, its place and its class. The plant
`tpm-response-over-the-frame-bound` takes away the bound of the frame.
