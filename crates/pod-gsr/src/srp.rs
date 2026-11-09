//! The TLS session with SRP.
//!
//! OpenSSL has the cipher suite that gsocket uses, and rustls has none, so
//! the crate builds the context with the five SRP functions. The server
//! gives the password to a callback that computes the verifier; the client
//! names the user and gives the password.

use std::ffi::{CString, c_char, c_int, c_void};

use openssl::ssl::{Ssl, SslContext, SslContextBuilder, SslMethod, SslOptions, SslVersion};
use openssl_sys::{SSL, SSL_CTX};
use pod_core::{Class, Failure, Stage};
use zeroize::Zeroizing;

use crate::identity::Identity;
use crate::settings::Settings;

/// The side of the TLS handshake that the relay named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// This peer proves itself with the verifier of the password.
    Server,
    /// This peer proves itself with the password.
    Client,
}

/// The alert that OpenSSL sends when the server has no SRP parameters.
const SSL_AD_INTERNAL_ERROR: c_int = 80;

/// The reason of a read that found the transport closed with no close_notify.
#[cfg(feature = "rt")]
pub(crate) const SSL_R_UNEXPECTED_EOF_WHILE_READING: c_int = 294;

unsafe extern "C" {
    fn SSL_CTX_set_srp_username(ctx: *mut SSL_CTX, name: *mut c_char) -> c_int;
    fn SSL_CTX_set_srp_password(ctx: *mut SSL_CTX, password: *mut c_char) -> c_int;
    fn SSL_CTX_set_srp_username_callback(
        ctx: *mut SSL_CTX,
        callback: Option<unsafe extern "C" fn(*mut SSL, *mut c_int, *mut c_void) -> c_int>,
    ) -> c_int;
    fn SSL_CTX_set_srp_cb_arg(ctx: *mut SSL_CTX, arg: *mut c_void) -> c_int;
    fn SSL_set_srp_server_param_pw(
        ssl: *mut SSL,
        user: *const c_char,
        password: *const c_char,
        group: *const c_char,
    ) -> c_int;
}

/// The strings that the SRP functions read. The context of the server keeps
/// a pointer to this value, so the value is boxed and lives for the session.
struct Params {
    user: CString,
    /// The password and a NUL byte, wiped when the session drops.
    password: Zeroizing<Vec<u8>>,
    group: CString,
}

impl Params {
    fn password_ptr(&self) -> *const c_char {
        self.password.as_ptr().cast()
    }
}

/// The callback of the server: it sets the verifier of the password on the
/// session. `arg` is the `Params` of the context.
///
/// The return value is the SSL error code of the callback: 0 for success, and
/// an alert for a failure.
unsafe extern "C" fn server_params(ssl: *mut SSL, _alert: *mut c_int, arg: *mut c_void) -> c_int {
    if ssl.is_null() || arg.is_null() {
        return SSL_AD_INTERNAL_ERROR;
    }
    // SAFETY: the context that names this callback keeps the box of the
    // value alive for the session, and the value is never moved.
    let params = unsafe { &*(arg as *const Params) };
    let set =
        unsafe { SSL_set_srp_server_param_pw(ssl, params.user.as_ptr(), params.password_ptr(), params.group.as_ptr()) };
    if set == 1 { 0 } else { SSL_AD_INTERNAL_ERROR }
}

/// The TLS context of one session.
pub(crate) struct Srp {
    context: SslContext,
    /// The context points into this value, so the field drops after the
    /// context. The field is never read again: it keeps the strings alive.
    _params: Box<Params>,
}

impl Srp {
    /// The context of one side of a session.
    pub(crate) fn new(settings: &Settings, identity: &Identity, side: Side) -> Result<Srp, Failure> {
        let mut builder = SslContextBuilder::new(SslMethod::tls()).map_err(setup)?;
        // TLS 1.3 has no SRP suite. Both sides are held to TLS 1.2: the
        // loopback tests pair with this cap on OpenSSL 3.6.3, and so do a cap
        // on the server alone and no cap; the live run through the relay used
        // this one.
        builder.set_min_proto_version(Some(SslVersion::TLS1_2)).map_err(setup)?;
        builder.set_max_proto_version(Some(SslVersion::TLS1_2)).map_err(setup)?;
        builder.set_cipher_list(&settings.cipher).map_err(setup)?;
        builder.set_options(SslOptions::NO_TICKET | SslOptions::NO_RENEGOTIATION);

        let secret = identity.srp_secret(settings);
        if secret.as_bytes().contains(&0) {
            return Err(nul_failure());
        }
        let mut password = Zeroizing::new(Vec::with_capacity(secret.len() + 1));
        password.extend_from_slice(secret.as_bytes());
        password.push(0);
        let params = Box::new(Params {
            user: CString::new(settings.srp_user.as_str()).map_err(nul)?,
            password,
            group: CString::new(settings.srp_group.as_str()).map_err(nul)?,
        });

        // SAFETY: the builder owns a live context, and `params` outlives it.
        let configured = unsafe {
            let context = builder.as_ptr();
            match side {
                Side::Server => {
                    let arg = &*params as *const Params as *mut c_void;
                    SSL_CTX_set_srp_username_callback(context, Some(server_params)) == 1
                        && SSL_CTX_set_srp_cb_arg(context, arg) == 1
                }
                Side::Client => {
                    SSL_CTX_set_srp_username(context, params.user.as_ptr() as *mut c_char) == 1
                        && SSL_CTX_set_srp_password(context, params.password_ptr() as *mut c_char) == 1
                }
            }
        };
        if !configured {
            return Err(Failure::new(Stage::Tls, Class::Config, "OpenSSL refused the SRP configuration"));
        }
        Ok(Srp { context: builder.build(), _params: params })
    }

    /// The TLS object of one connection.
    pub(crate) fn ssl(&self) -> Result<Ssl, Failure> {
        Ssl::new(&self.context).map_err(setup)
    }
}

fn setup(error: openssl::error::ErrorStack) -> Failure {
    Failure::new(Stage::Tls, Class::Config, format!("OpenSSL refused the SRP configuration: {error}"))
}

fn nul(_error: std::ffi::NulError) -> Failure {
    nul_failure()
}

fn nul_failure() -> Failure {
    Failure::new(Stage::Tls, Class::Config, "a value of the SRP configuration holds a NUL byte")
}

/// Build the TLS context of each side for a made-up secret, and give the
/// version text of the linked OpenSSL. It checks that OpenSSL has the SRP
/// functions, the cipher suite and the group of `settings`, with no
/// connection: a self-test of a binary.
pub fn check_tls(settings: &Settings) -> Result<&'static str, Failure> {
    let identity = Identity::new("pod-gsr-check", settings);
    for side in [Side::Server, Side::Client] {
        Srp::new(settings, &identity, side)?.ssl()?;
    }
    Ok(openssl::version::version())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::of_data().expect("the data file")
    }

    #[test]
    fn the_check_names_an_openssl_3() {
        let version = check_tls(&settings()).expect("the contexts of both sides");
        assert!(version.starts_with("OpenSSL 3."), "{version}");
    }

    #[test]
    fn a_cipher_that_openssl_does_not_have_is_a_configuration_failure() {
        let mut settings = settings();
        settings.cipher = "NO-SUCH-CIPHER".into();
        let failure = check_tls(&settings).expect_err("no cipher");
        assert_eq!((failure.stage, failure.class), (Stage::Tls, Class::Config));
    }

    #[test]
    fn a_context_builds_for_each_side() {
        let settings = settings();
        let identity = Identity::new("testsecret123", &settings);
        assert!(Srp::new(&settings, &identity, Side::Server).is_ok());
        assert!(Srp::new(&settings, &identity, Side::Client).is_ok());
    }
}
