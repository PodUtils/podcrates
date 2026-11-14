//! `transit-relay`: magic-wormhole's transit relay over TCP pairs two
//! connections of one token and carries bytes between them. Each end sends
//! `please relay TOKEN for side SIDE\n` (TOKEN 64 hex digits, SIDE 16); the
//! relay answers `ok\n` to both when the second end comes, and from then on
//! gives each end the bytes of the other
//! (magic-wormhole/magic-wormhole-transit-relay:src/wormhole_transit_relay/transit_server.py).
//! The watch is both ends: a relay that answers is one that a transfer can
//! take, not only a port that is open.

use pod_core::BoxedIo;
use pod_core::entropy::{Entropy, OsEntropy};
use pod_services::{Service, Watch};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::args::Args;
use crate::{Checked, Finding, Net, Outcome};

/// The answer of the relay when the two ends are paired.
const OK: &[u8] = b"ok\n";
/// The most bytes of an answer: `ok`, `impatient` or `bad handshake`.
const ANSWER_MOST: usize = 64;

/// The handshake of one end: `please relay TOKEN for side SIDE\n`.
pub fn handshake(token: &[u8; 32], side: &[u8; 8]) -> String {
    format!("please relay {} for side {}\n", hex(token), hex(side))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random<const N: usize>() -> Result<[u8; N], Finding> {
    let mut bytes = [0u8; N];
    OsEntropy.fill(&mut bytes).map_err(|_| Finding::failed("the system gave no random bytes"))?;
    Ok(bytes)
}

/// `transit-relay`: two connections to the endpoint (scheme `tcp`) with one
/// random token and two sides each get `ok`, and bytes that the first sends
/// come out of the second.
pub async fn relay(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let endpoint = Args::new(service, watch).endpoint()?;
    if endpoint.scheme() != "tcp" {
        let why = format!("the endpoint {} is of the scheme {}, not tcp", endpoint.name(), endpoint.scheme());
        return Err(Finding::failed(why));
    }
    let at = endpoint.authority();
    let (token, one_side, two_side) = (random::<32>()?, random::<8>()?, random::<8>()?);
    let mut one = net.tcp(endpoint.host(), endpoint.port()).await?;
    one.write_all(handshake(&token, &one_side).as_bytes()).await.map_err(|e| sent(&at, &e))?;
    let mut two = net.tcp(endpoint.host(), endpoint.port()).await?;
    two.write_all(handshake(&token, &two_side).as_bytes()).await.map_err(|e| sent(&at, &e))?;
    let paired = async {
        answer(&mut one, &at).await?;
        answer(&mut two, &at).await
    };
    tokio::time::timeout(net.limit, paired)
        .await
        .map_err(|_| Finding::failed(format!("{at} did not pair the two ends within {:?}", net.limit)))??;
    let probe = format!("pod-services-watch {}\n", hex(&random::<8>()?));
    one.write_all(probe.as_bytes()).await.map_err(|e| sent(&at, &e))?;
    let mut got = vec![0u8; probe.len()];
    let carried = tokio::time::timeout(net.limit, two.read_exact(&mut got)).await;
    match carried {
        Ok(Ok(_)) if got == probe.as_bytes() => {}
        Ok(Ok(_)) => return Err(Finding::failed(format!("{at} gave the second end other bytes than the first sent"))),
        Ok(Err(e)) => return Err(Finding::failed(format!("{at} ended before it carried the bytes: {e}"))),
        Err(_) => return Err(Finding::failed(format!("{at} carried no bytes within {:?}", net.limit))),
    }
    let mut out = Outcome::default();
    out.saw(format!("{at} paired two ends of one token and carried {} bytes", probe.len()));
    Ok(out)
}

fn sent(at: &str, e: &std::io::Error) -> Finding {
    Finding::failed(format!("the handshake to {at}: {e}"))
}

/// The answer of the relay to one end: `ok`, or a failure with its words.
async fn answer(io: &mut BoxedIo, at: &str) -> Result<(), Finding> {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while bytes.len() < ANSWER_MOST {
        match io.read(&mut byte).await {
            Ok(0) => break,
            Ok(_) => bytes.push(byte[0]),
            Err(e) => return Err(Finding::failed(format!("{at} ended the handshake: {e}"))),
        }
        if byte[0] == b'\n' {
            break;
        }
    }
    if bytes == OK {
        return Ok(());
    }
    let said = pod_core::text::one_line(&String::from_utf8_lossy(&bytes));
    Err(Finding::failed(format!("{at} answered {said:?} to the handshake, not \"ok\"")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handshake_is_the_relays_line_of_64_and_16_hex_digits() {
        let line = handshake(&[0xab; 32], &[1; 8]);
        assert_eq!(line, format!("please relay {} for side 0101010101010101\n", "ab".repeat(32)));
    }
}
