//! The TPM Base Services of Windows (`tbs.dll`, Windows Vista and later):
//! `Tbsi_Context_Create` for a context of a TPM 2.0, `Tbsip_Submit_Command`
//! for each command, `Tbsip_Context_Close` on drop. The functions come from
//! `windows-sys` as `raw-dylib` imports: the build needs no import library
//! and no C, and `tbs.dll` is a DLL of the system.
//!
//! The map of the codes of TBS to a [`Failure`] ([`failure`], [`name`]) is
//! on each system, so that its tests run on each host; the transport `Tbs`
//! is on Windows alone.

use pod_core::{Class, Failure, Stage};

/// The codes of TBS (`tbs.h`, `winerror.h`), as `windows-sys` gives them
/// in `Win32::Foundation`: the Windows build checks each one against it.
pub const TBS_E_INTERNAL_ERROR: u32 = 0x8028_4001;
pub const TBS_E_BAD_PARAMETER: u32 = 0x8028_4002;
pub const TBS_E_INVALID_OUTPUT_POINTER: u32 = 0x8028_4003;
pub const TBS_E_INVALID_CONTEXT: u32 = 0x8028_4004;
pub const TBS_E_INSUFFICIENT_BUFFER: u32 = 0x8028_4005;
pub const TBS_E_IOERROR: u32 = 0x8028_4006;
pub const TBS_E_INVALID_CONTEXT_PARAM: u32 = 0x8028_4007;
pub const TBS_E_SERVICE_NOT_RUNNING: u32 = 0x8028_4008;
pub const TBS_E_TOO_MANY_TBS_CONTEXTS: u32 = 0x8028_4009;
pub const TBS_E_TOO_MANY_RESOURCES: u32 = 0x8028_400A;
pub const TBS_E_SERVICE_START_PENDING: u32 = 0x8028_400B;
pub const TBS_E_PPI_NOT_SUPPORTED: u32 = 0x8028_400C;
pub const TBS_E_COMMAND_CANCELED: u32 = 0x8028_400D;
pub const TBS_E_BUFFER_TOO_LARGE: u32 = 0x8028_400E;
pub const TBS_E_TPM_NOT_FOUND: u32 = 0x8028_400F;
pub const TBS_E_SERVICE_DISABLED: u32 = 0x8028_4010;
pub const TBS_E_NO_EVENT_LOG: u32 = 0x8028_4011;
pub const TBS_E_ACCESS_DENIED: u32 = 0x8028_4012;
pub const TBS_E_PROVISIONING_NOT_ALLOWED: u32 = 0x8028_4013;
pub const TBS_E_PPI_FUNCTION_UNSUPPORTED: u32 = 0x8028_4014;
pub const TBS_E_OWNERAUTH_NOT_FOUND: u32 = 0x8028_4015;
pub const TBS_E_PROVISIONING_INCOMPLETE: u32 = 0x8028_4016;

/// Each code: its name, its class and its words.
const CODES: [(u32, &str, Class, &str); 22] = [
    (TBS_E_INTERNAL_ERROR, "TBS_E_INTERNAL_ERROR", Class::Internal, "an internal error of TBS"),
    (TBS_E_BAD_PARAMETER, "TBS_E_BAD_PARAMETER", Class::Usage, "a parameter is not correct"),
    (TBS_E_INVALID_OUTPUT_POINTER, "TBS_E_INVALID_OUTPUT_POINTER", Class::Internal, "an output pointer is not correct"),
    (TBS_E_INVALID_CONTEXT, "TBS_E_INVALID_CONTEXT", Class::Usage, "the context is not correct"),
    (TBS_E_INSUFFICIENT_BUFFER, "TBS_E_INSUFFICIENT_BUFFER", Class::Protocol, "the response is larger than 4096 bytes"),
    (TBS_E_IOERROR, "TBS_E_IOERROR", Class::Unavailable, "an error of I/O between TBS and the TPM"),
    (TBS_E_INVALID_CONTEXT_PARAM, "TBS_E_INVALID_CONTEXT_PARAM", Class::Unsupported, "TBS has no context of a TPM 2.0"),
    (
        TBS_E_SERVICE_NOT_RUNNING,
        "TBS_E_SERVICE_NOT_RUNNING",
        Class::Unavailable,
        "no TPM: the service TBS does not run",
    ),
    (TBS_E_TOO_MANY_TBS_CONTEXTS, "TBS_E_TOO_MANY_TBS_CONTEXTS", Class::TempFail, "too many contexts are open"),
    (TBS_E_TOO_MANY_RESOURCES, "TBS_E_TOO_MANY_RESOURCES", Class::TempFail, "the context holds too many resources"),
    (TBS_E_SERVICE_START_PENDING, "TBS_E_SERVICE_START_PENDING", Class::TempFail, "the service TBS starts"),
    (TBS_E_PPI_NOT_SUPPORTED, "TBS_E_PPI_NOT_SUPPORTED", Class::Unsupported, "no interface of physical presence"),
    (TBS_E_COMMAND_CANCELED, "TBS_E_COMMAND_CANCELED", Class::TempFail, "the command was cancelled"),
    (TBS_E_BUFFER_TOO_LARGE, "TBS_E_BUFFER_TOO_LARGE", Class::Usage, "the command is too large"),
    (TBS_E_TPM_NOT_FOUND, "TBS_E_TPM_NOT_FOUND", Class::Unavailable, "no TPM: TBS found no TPM 2.0"),
    (TBS_E_SERVICE_DISABLED, "TBS_E_SERVICE_DISABLED", Class::Unavailable, "no TPM: the service TBS is disabled"),
    (TBS_E_NO_EVENT_LOG, "TBS_E_NO_EVENT_LOG", Class::Unavailable, "no event log"),
    (TBS_E_ACCESS_DENIED, "TBS_E_ACCESS_DENIED", Class::Permission, "this user may not send the command"),
    (
        TBS_E_PROVISIONING_NOT_ALLOWED,
        "TBS_E_PROVISIONING_NOT_ALLOWED",
        Class::Permission,
        "provisioning is not allowed",
    ),
    (TBS_E_PPI_FUNCTION_UNSUPPORTED, "TBS_E_PPI_FUNCTION_UNSUPPORTED", Class::Unsupported, "no such physical presence"),
    (TBS_E_OWNERAUTH_NOT_FOUND, "TBS_E_OWNERAUTH_NOT_FOUND", Class::Unavailable, "no authorization of the owner"),
    (TBS_E_PROVISIONING_INCOMPLETE, "TBS_E_PROVISIONING_INCOMPLETE", Class::Unavailable, "the TPM is not provisioned"),
];

/// The name of a code of TBS, when the table knows it.
pub fn name(code: u32) -> Option<&'static str> {
    CODES.iter().find(|c| c.0 == code).map(|c| c.1)
}

/// The failure of the call `call` that gave the code `code`: the class of
/// its meaning (`Unavailable` for no TPM and for a code that the table does
/// not know, `Permission` for a refusal), its words, its name and its
/// number.
pub fn failure(call: &str, code: u32) -> Failure {
    let text = match CODES.iter().find(|c| c.0 == code) {
        Some(&(_, name, class, words)) => {
            return Failure::new(Stage::Host, class, format!("{call} of TBS: {words} ({name}, {code:#010x})"));
        }
        None => format!("{call} of TBS gave the code {code:#010x}"),
    };
    Failure::new(Stage::Host, Class::Unavailable, text)
}

#[cfg(windows)]
pub use windows::Tbs;

#[cfg(windows)]
mod windows {
    use core::ffi::c_void;

    use pod_core::{Class, Failure, Stage};
    use windows_sys::Win32::Foundation as F;
    use windows_sys::Win32::System::TpmBaseServices as T;

    use super::failure;
    use crate::transport::{Transport, check_command};
    use crate::wire::MAX_RESPONSE;

    // The codes of the table are those of windows-sys.
    macro_rules! same {
        ($($name:ident),*) => { $(const _: () = assert!(super::$name == F::$name as u32);)* };
    }
    same!(
        TBS_E_INTERNAL_ERROR,
        TBS_E_BAD_PARAMETER,
        TBS_E_INVALID_OUTPUT_POINTER,
        TBS_E_INVALID_CONTEXT,
        TBS_E_INSUFFICIENT_BUFFER,
        TBS_E_IOERROR,
        TBS_E_INVALID_CONTEXT_PARAM,
        TBS_E_SERVICE_NOT_RUNNING,
        TBS_E_TOO_MANY_TBS_CONTEXTS,
        TBS_E_TOO_MANY_RESOURCES,
        TBS_E_SERVICE_START_PENDING,
        TBS_E_PPI_NOT_SUPPORTED,
        TBS_E_COMMAND_CANCELED,
        TBS_E_BUFFER_TOO_LARGE,
        TBS_E_TPM_NOT_FOUND,
        TBS_E_SERVICE_DISABLED,
        TBS_E_NO_EVENT_LOG,
        TBS_E_ACCESS_DENIED,
        TBS_E_PROVISIONING_NOT_ALLOWED,
        TBS_E_PPI_FUNCTION_UNSUPPORTED,
        TBS_E_OWNERAUTH_NOT_FOUND,
        TBS_E_PROVISIONING_INCOMPLETE
    );

    /// The bit `includeTpm20` of `TBS_CONTEXT_PARAMS2`.
    const INCLUDE_TPM20: u32 = 1 << 2;

    /// A context of TBS for a TPM 2.0, open as long as the caller holds it.
    /// `Tbsip_Submit_Command` blocks until the TPM answers: TBS has no time
    /// limit that the caller sets, and this transport does not call
    /// `Tbsip_Cancel_Commands`.
    pub struct Tbs {
        context: *mut c_void,
    }

    impl Tbs {
        /// Open a context of a TPM 2.0. No TPM, or a TPM 1.2 alone, is the
        /// class `Unavailable`.
        pub fn open() -> Result<Tbs, Failure> {
            let params = T::TBS_CONTEXT_PARAMS2 {
                version: T::TBS_CONTEXT_VERSION_TWO,
                Anonymous: T::TBS_CONTEXT_PARAMS2_0 { asUINT32: INCLUDE_TPM20 },
            };
            let mut context: *mut c_void = core::ptr::null_mut();
            // SAFETY: the parameters of version two live for the call, and
            // TBS writes one handle to `context`.
            let code =
                unsafe { T::Tbsi_Context_Create((&raw const params).cast::<T::TBS_CONTEXT_PARAMS>(), &mut context) };
            if code != T::TBS_SUCCESS {
                return Err(failure("Tbsi_Context_Create", code));
            }
            Ok(Tbs { context })
        }
    }

    impl Transport for Tbs {
        fn submit(&mut self, command: &[u8]) -> Result<Vec<u8>, Failure> {
            check_command(command)?;
            let mut answer = vec![0u8; MAX_RESPONSE];
            let mut size = MAX_RESPONSE as u32;
            // SAFETY: the command and the buffer live for the call; TBS
            // writes at most `size` bytes and the count to `size`.
            let code = unsafe {
                T::Tbsip_Submit_Command(
                    self.context,
                    T::TBS_COMMAND_LOCALITY_ZERO,
                    T::TBS_COMMAND_PRIORITY_NORMAL,
                    command.as_ptr(),
                    command.len() as u32,
                    answer.as_mut_ptr(),
                    &mut size,
                )
            };
            if code != T::TBS_SUCCESS {
                return Err(failure("Tbsip_Submit_Command", code));
            }
            let size = size as usize;
            if size > MAX_RESPONSE {
                let why = format!("TBS says a response of {size} bytes, more than {MAX_RESPONSE}");
                return Err(Failure::new(Stage::Host, Class::Protocol, why));
            }
            answer.truncate(size);
            Ok(answer)
        }
    }

    impl Drop for Tbs {
        fn drop(&mut self) {
            // SAFETY: the context of `open`, closed once.
            let _ = unsafe { T::Tbsip_Context_Close(self.context) };
        }
    }
}
