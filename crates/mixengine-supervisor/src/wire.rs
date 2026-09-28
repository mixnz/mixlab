//! Two health probes that speak a database's own protocol — roadmap task **T190b**, the spec's D4.
//!
//! **Instead of starting `mysqladmin ping` and `redis-cli ping`.** On Windows a process start is a
//! restricted token, `CreateProcessAsUser` and a thread per pipe: 10 ms of the daemon's CPU, twice
//! every ten seconds, to learn whether two servers answer. Each question here is one connection and
//! a few bytes. The judgement of those bytes is a pure function, so it is tested without a socket.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

/// The most of a reply either probe reads. A greeting's header and first byte are five bytes, and
/// a Redis status line is short: a server that sends more has already answered.
const ENOUGH: usize = 64;

/// How many bytes a MySQL greeting has to show before it can be judged: a header and one byte.
const GREETING_HEAD: usize = 5;

/// Whether these bytes open a MySQL packet a server sends before any login.
///
/// A packet is a three-byte little-endian length, a sequence byte, then the payload. The first
/// payload byte of a greeting is the protocol version, `0x0a`. A server refusing the connection —
/// *too many connections*, *host blocked* — sends an error packet, `0xff`, and is answering.
pub(crate) fn mysql_greets(bytes: &[u8]) -> bool {
    let [a, b, c, _sequence, first, ..] = *bytes else {
        return false;
    };

    let length = u32::from_le_bytes([a, b, c, 0]);

    length > 0 && matches!(first, 0x0a | 0xff)
}

/// Whether this is a Redis server answering `PING`.
///
/// `+PONG` is, and so is any error but `-LOADING`: `-NOAUTH` is a server that answered, where
/// `-LOADING` is one that will not serve reads yet.
pub(crate) fn redis_answers(line: &[u8]) -> bool {
    match line.first() {
        Some(b'+') => line.starts_with(b"+PONG"),
        Some(b'-') => !line.starts_with(b"-LOADING"),
        _ => false,
    }
}

/// Read what the server says first, within `timeout`, and judge it.
///
/// A timeout, a refused connection and a server that closes without a word are all `false`.
pub(crate) async fn greeting(addr: SocketAddr, timeout: Duration) -> bool {
    let read = tokio::time::timeout(timeout, async {
        let mut stream = TcpStream::connect(addr).await.ok()?;

        read_until(&mut stream, |seen| seen.len() >= GREETING_HEAD).await
    })
    .await;

    matches!(read, Ok(Some(bytes)) if mysql_greets(&bytes))
}

/// Send `PING` and judge the first reply line, within `timeout`.
pub(crate) async fn ping(addr: SocketAddr, timeout: Duration) -> bool {
    let read = tokio::time::timeout(timeout, async {
        let mut stream = TcpStream::connect(addr).await.ok()?;
        stream.write_all(b"*1\r\n$4\r\nPING\r\n").await.ok()?;

        read_until(&mut stream, |seen| seen.contains(&b'\n')).await
    })
    .await;

    matches!(read, Ok(Some(line)) if redis_answers(&line))
}

/// Read until `enough` says so, the peer closes, or [`ENOUGH`] bytes have arrived.
///
/// [`None`] only for a read that failed; a peer that closed early answers with what it sent, which
/// the judge then refuses. A reply may arrive in more than one segment, which is why this loops.
async fn read_until(stream: &mut TcpStream, enough: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    let mut buffer = [0_u8; ENOUGH];
    let mut filled = 0;

    while filled < ENOUGH && !enough(&buffer[..filled]) {
        let read = stream.read(&mut buffer[filled..]).await.ok()?;
        if read == 0 {
            break;
        }
        filled += read;
    }

    Some(buffer[..filled].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handshake_is_a_greeting() {
        // Length 74, sequence 0, protocol version 10.
        assert!(mysql_greets(&[0x4a, 0x00, 0x00, 0x00, 0x0a, b'8']));
    }

    #[test]
    fn an_error_packet_is_a_server_answering() {
        // "Too many connections" arrives as an error packet, before any login.
        assert!(mysql_greets(&[0x17, 0x00, 0x00, 0x00, 0xff, 0x10, 0x04]));
    }

    #[test]
    fn a_truncated_header_is_not_a_greeting() {
        assert!(!mysql_greets(&[0x4a, 0x00]));
        assert!(!mysql_greets(&[]));
    }

    #[test]
    fn another_protocol_is_not_a_greeting() {
        assert!(!mysql_greets(b"SSH-2.0-OpenSSH"));
        assert!(
            !mysql_greets(&[0x00, 0x00, 0x00, 0x00, 0x0a]),
            "a zero length is no packet"
        );
    }

    #[test]
    fn pong_and_most_errors_are_answers() {
        assert!(redis_answers(b"+PONG\r\n"));
        assert!(redis_answers(b"-NOAUTH Authentication required.\r\n"));
        assert!(redis_answers(b"-ERR unknown command\r\n"));
    }

    #[test]
    fn loading_and_nothing_are_not_answers() {
        assert!(!redis_answers(
            b"-LOADING Redis is loading the dataset in memory\r\n"
        ));
        assert!(!redis_answers(b""));
        assert!(!redis_answers(b"HTTP/1.1 400 Bad Request\r\n"));
    }
}
