//! The client side of Microsoft Dev Tunnels, as the vendor's open-source
//! client speaks it (microsoft/dev-tunnels, `rs/`): the connect
//! information of a tunnel from the management API with the user's token
//! ([`Management`]), the relay WebSocket with its subprotocol through the
//! caller's TLS, proxy and dial ([`Way`]), SSH over it to the host of the
//! tunnel with no user authentication, and a `direct-tcpip` channel to a
//! port of the host as a byte stream ([`Relay::open`], [`Stream`]), with a
//! bounded reconnect that takes a fresh connect token.
//!
//! Each value of the protocol is a fact of the service
//! `microsoft-devtunnel` of `pod-services`, with its source ([`service`]).
//! The crate reads no variable of the environment, keeps no process state
//! and prints nothing; a line for the user is a [`pod_core::Event`]. A
//! token is in no `Debug`, no line, no event and no error text. Each
//! failure is a [`pod_core::Failure`] whose chain names its [`Step`]
//! ([`step_of`]).
//!
//! The host side, the creation, the list and the delete of tunnels, the
//! device-code logins and a token store are not in this crate (T-378, the
//! second part).

pub mod failure;
pub mod info;
mod live;
pub mod management;
pub mod relay;
pub mod service;
pub mod settings;
pub mod ssh;
#[cfg(feature = "standin")]
pub mod standin;
pub mod stream;
pub mod token;

pub use failure::{Failed, Step, step_of, steps_of};
pub use info::{ConnectInfo, Endpoint, Tunnel};
pub use management::{Locator, Management};
pub use relay::Relay;
pub use settings::{Base, Settings, Way};
pub use stream::Stream;
pub use token::{ConnectToken, Scheme, UserToken};
