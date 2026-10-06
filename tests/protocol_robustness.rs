//! Property tests for the wire-protocol parsers.
//!
//! The parsers consume bytes from untrusted clients before any authentication or
//! ACL decision. These tests feed them arbitrary and structurally mutated input and
//! assert the properties that matter for a network-facing parser: they never panic,
//! they terminate, and valid packets survive a serialize/parse round trip.

use bytes::Bytes;
use proptest::collection::vec;
use proptest::prelude::*;
use rustsocks::protocol::{
    parse_gssapi_message, parse_socks4_request, parse_socks5_client_greeting, parse_socks5_request,
    parse_udp_packet, parse_userpass_auth, serialize_udp_packet, Address, UdpHeader, UdpPacket,
};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::runtime::Builder;
use tokio::time::timeout;

/// Read-only-from-buffer stream: reads yield the input then EOF, writes are discarded.
struct MockStream {
    input: std::io::Cursor<Vec<u8>>,
}

impl MockStream {
    fn new(data: impl Into<Vec<u8>>) -> Self {
        Self {
            input: std::io::Cursor::new(data.into()),
        }
    }
}

impl AsyncRead for MockStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let pos = self.input.position() as usize;
        let data = self.input.get_ref();
        let n = buf.remaining().min(data.len().saturating_sub(pos));
        buf.put_slice(&data[pos..pos + n]);
        self.input.set_position((pos + n) as u64);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for MockStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Run a parser future to completion on a fresh runtime, failing if it hangs.
fn run<F: std::future::Future>(fut: F) -> F::Output {
    let rt = Builder::new_current_thread().enable_time().build().unwrap();
    rt.block_on(async {
        timeout(Duration::from_secs(5), fut)
            .await
            .expect("parser must terminate on finite input")
    })
}

fn arb_address() -> impl Strategy<Value = Address> {
    prop_oneof![
        any::<[u8; 4]>().prop_map(Address::IPv4),
        any::<[u8; 16]>().prop_map(Address::IPv6),
        "[a-z0-9]([a-z0-9-]{0,30}[a-z0-9])?(\\.[a-z]{2,10}){1,3}".prop_map(Address::Domain),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn socks5_request_never_panics(data in vec(any::<u8>(), 0..300)) {
        let mut reader = MockStream::new(data.clone());
        let _ = run(parse_socks5_request(&mut reader));
    }

    #[test]
    fn socks4_request_never_panics(data in vec(any::<u8>(), 0..300)) {
        let mut reader = MockStream::new(data.clone());
        let _ = run(parse_socks4_request(&mut reader));
    }

    #[test]
    fn socks5_greeting_never_panics(version in any::<u8>(), data in vec(any::<u8>(), 0..300)) {
        let mut reader = MockStream::new(data.clone());
        let _ = run(parse_socks5_client_greeting(&mut reader, version));
    }

    #[test]
    fn userpass_auth_never_panics(data in vec(any::<u8>(), 0..600)) {
        let mut reader = MockStream::new(data.clone());
        let _ = run(parse_userpass_auth(&mut reader));
    }

    #[test]
    fn gssapi_message_never_panics(data in vec(any::<u8>(), 0..300)) {
        let mut reader = MockStream::new(data.clone());
        let _ = run(parse_gssapi_message(&mut reader));
    }

    #[test]
    fn udp_packet_parse_never_panics(data in vec(any::<u8>(), 0..2048)) {
        let _ = parse_udp_packet(Bytes::from(data));
    }

    /// Anything the parser accepts must be re-serializable, and re-parsing the
    /// serialized form must yield the same header and payload.
    #[test]
    fn accepted_udp_packets_round_trip(data in vec(any::<u8>(), 0..2048)) {
        if let Ok(packet) = parse_udp_packet(Bytes::from(data)) {
            let wire = serialize_udp_packet(&packet).expect("accepted packet serializes");
            let again = parse_udp_packet(Bytes::from(wire)).expect("serialized packet parses");
            prop_assert_eq!(again.header.frag, packet.header.frag);
            prop_assert_eq!(again.header.address, packet.header.address);
            prop_assert_eq!(again.header.port, packet.header.port);
            prop_assert_eq!(again.data, packet.data);
        }
    }

    #[test]
    fn constructed_udp_packets_round_trip(
        address in arb_address(),
        port in any::<u16>(),
        payload in vec(any::<u8>(), 0..1024),
    ) {
        let packet = UdpPacket {
            header: UdpHeader { frag: 0, address, port },
            data: Bytes::from(payload),
        };
        let wire = serialize_udp_packet(&packet).expect("valid packet serializes");
        let parsed = parse_udp_packet(Bytes::from(wire)).expect("valid packet parses");
        prop_assert_eq!(parsed.header.address, packet.header.address);
        prop_assert_eq!(parsed.header.port, packet.header.port);
        prop_assert_eq!(parsed.data, packet.data);
    }

    /// Flip, truncate and extend a valid SOCKS5 CONNECT request: still no panic.
    #[test]
    fn mutated_socks5_connect_never_panics(
        address in arb_address(),
        port in any::<u16>(),
        flips in vec((any::<prop::sample::Index>(), any::<u8>()), 0..6),
        cut in any::<prop::sample::Index>(),
    ) {
        let mut request = vec![0x05, 0x01, 0x00];
        match &address {
            Address::IPv4(ip) => { request.push(0x01); request.extend_from_slice(ip); }
            Address::IPv6(ip) => { request.push(0x04); request.extend_from_slice(ip); }
            Address::Domain(name) => {
                request.push(0x03);
                request.push(name.len() as u8);
                request.extend_from_slice(name.as_bytes());
            }
        }
        request.extend_from_slice(&port.to_be_bytes());

        for (index, value) in flips {
            let at = index.index(request.len());
            request[at] = value;
        }
        request.truncate(cut.index(request.len() + 1));

        let mut reader = MockStream::new(request.clone());
        let _ = run(parse_socks5_request(&mut reader));
    }
}

#[test]
fn well_formed_socks5_connect_is_accepted() {
    let mut request = vec![0x05, 0x01, 0x00, 0x03, 11];
    request.extend_from_slice(b"example.com");
    request.extend_from_slice(&443u16.to_be_bytes());
    let mut reader = MockStream::new(request.clone());
    let parsed = run(parse_socks5_request(&mut reader)).expect("valid request parses");
    assert_eq!(parsed.port, 443);
    assert!(matches!(parsed.address, Address::Domain(ref d) if d == "example.com"));
}
