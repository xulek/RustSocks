// Protocol Parser Edge Cases Tests
// Tests for malformed input, boundary conditions, and error handling

use rustsocks::protocol::parser::*;
use rustsocks::protocol::types::*;
use rustsocks::utils::error::RustSocksError;
use std::io::Cursor;

// Helper to create a mock stream for testing
struct MockStream {
    read_buf: Cursor<Vec<u8>>,
    write_buf: Vec<u8>,
}

impl MockStream {
    fn new(data: Vec<u8>) -> Self {
        Self {
            read_buf: Cursor::new(data),
            write_buf: Vec::new(),
        }
    }
}

impl tokio::io::AsyncRead for MockStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let inner = &mut self.read_buf;
        std::pin::Pin::new(inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for MockStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        self.write_buf.extend_from_slice(buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn test_client_greeting_max_methods() {
    // Test with maximum number of methods (255)
    let mut data = vec![255u8]; // nmethods = 255
    for i in 0..255 {
        data.push(i); // Add 255 different method codes
    }

    let mut stream = MockStream::new(data);
    let result = parse_socks5_client_greeting(&mut stream, SOCKS_VERSION).await;

    assert!(result.is_ok());
    let greeting = result.unwrap();
    assert_eq!(greeting.methods.len(), 255);
}

#[tokio::test]
async fn test_client_greeting_unsupported_version() {
    // Test with SOCKS4 version
    let mut stream = MockStream::new(vec![1, 0x00]); // 1 method
    let result = parse_socks5_client_greeting(&mut stream, 0x04).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RustSocksError::Protocol(msg) => {
            assert!(msg.contains("Unsupported SOCKS version"));
        }
        _ => panic!("Expected Protocol error"),
    }
}

#[tokio::test]
async fn test_client_greeting_zero_methods() {
    // Test with zero methods (should fail)
    let mut stream = MockStream::new(vec![0]); // nmethods = 0
    let result = parse_socks5_client_greeting(&mut stream, SOCKS_VERSION).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RustSocksError::Protocol(msg) => {
            assert!(msg.contains("No authentication methods provided"));
        }
        _ => panic!("Expected Protocol error"),
    }
}

#[tokio::test]
async fn test_client_greeting_incomplete_data() {
    // Test with incomplete method data
    let mut stream = MockStream::new(vec![5]); // nmethods = 5, but no method data
    let result = parse_socks5_client_greeting(&mut stream, SOCKS_VERSION).await;

    assert!(result.is_err());
    // Should get IO error due to unexpected EOF
}

#[tokio::test]
async fn test_userpass_auth_max_username_length() {
    // Test with maximum username length (255 chars)
    let mut data = vec![0x01]; // version
    data.push(255); // username length
    data.extend(vec![b'a'; 255]); // 255 'a' characters
    data.push(8); // password length
    data.extend(b"password");

    let mut stream = MockStream::new(data);
    let result = parse_userpass_auth(&mut stream).await;

    assert!(result.is_ok());
    let (username, password) = result.unwrap();
    assert_eq!(username.len(), 255);
    assert_eq!(password, "password");
}

#[tokio::test]
async fn test_userpass_auth_max_password_length() {
    // Test with maximum password length (255 chars)
    let mut data = vec![0x01]; // version
    data.push(5); // username length
    data.extend(b"alice");
    data.push(255); // password length
    data.extend(vec![b'x'; 255]); // 255 'x' characters

    let mut stream = MockStream::new(data);
    let result = parse_userpass_auth(&mut stream).await;

    assert!(result.is_ok());
    let (username, password) = result.unwrap();
    assert_eq!(username, "alice");
    assert_eq!(password.len(), 255);
}

#[tokio::test]
async fn test_userpass_auth_zero_length_username() {
    // Test with zero-length username
    let mut data = vec![0x01]; // version
    data.push(0); // username length = 0
    data.push(8); // password length
    data.extend(b"password");

    let mut stream = MockStream::new(data);
    let result = parse_userpass_auth(&mut stream).await;

    assert!(result.is_ok());
    let (username, password) = result.unwrap();
    assert_eq!(username, "");
    assert_eq!(password, "password");
}

#[tokio::test]
async fn test_userpass_auth_zero_length_password() {
    // Test with zero-length password
    let mut data = vec![0x01]; // version
    data.push(5); // username length
    data.extend(b"alice");
    data.push(0); // password length = 0

    let mut stream = MockStream::new(data);
    let result = parse_userpass_auth(&mut stream).await;

    assert!(result.is_ok());
    let (username, password) = result.unwrap();
    assert_eq!(username, "alice");
    assert_eq!(password, "");
}

#[tokio::test]
async fn test_userpass_auth_invalid_version() {
    // Test with invalid version (not 0x01)
    let mut data = vec![0x02]; // invalid version
    data.push(5);
    data.extend(b"alice");
    data.push(8);
    data.extend(b"password");

    let mut stream = MockStream::new(data);
    let result = parse_userpass_auth(&mut stream).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        RustSocksError::Protocol(msg) => {
            assert!(msg.contains("Unsupported userpass version"));
        }
        _ => panic!("Expected Protocol error"),
    }
}

#[tokio::test]
async fn test_userpass_auth_incomplete_username() {
    // Test with incomplete username data
    let mut data = vec![0x01]; // version
    data.push(10); // username length = 10
    data.extend(b"alice"); // but only 5 bytes

    let mut stream = MockStream::new(data);
    let result = parse_userpass_auth(&mut stream).await;

    assert!(result.is_err());
    // Should get IO error due to unexpected EOF
}

#[tokio::test]
async fn test_socks5_request_all_address_types() {
    // Test IPv4 request
    let mut data = vec![
        SOCKS_VERSION,
        Command::Connect as u8,
        0x00, // reserved
        0x01, // IPv4
    ];
    data.extend(&[192, 168, 1, 1]); // IP
    data.extend(&[0x00, 0x50]); // port 80

    let mut stream = MockStream::new(data);
    let result = parse_socks5_request(&mut stream).await;

    assert!(result.is_ok());
    let request = result.unwrap();
    assert_eq!(request.command, Command::Connect);
    assert!(matches!(request.address, Address::IPv4(_)));
    assert_eq!(request.port, 80);

    // Test IPv6 request
    let mut data = vec![
        SOCKS_VERSION,
        Command::Connect as u8,
        0x00,
        0x04, // IPv6
    ];
    data.extend(&[0u8; 16]); // IPv6 address
    data.extend(&[0x01, 0xBB]); // port 443

    let mut stream = MockStream::new(data);
    let result = parse_socks5_request(&mut stream).await;

    assert!(result.is_ok());
    let request = result.unwrap();
    assert!(matches!(request.address, Address::IPv6(_)));
    assert_eq!(request.port, 443);

    // Test Domain request with max length (255)
    let mut data = vec![
        SOCKS_VERSION,
        Command::Connect as u8,
        0x00,
        0x03, // Domain
        255,  // domain length
    ];
    data.extend(vec![b'a'; 255]); // 255 character domain
    data.extend(&[0x00, 0x50]); // port 80

    let mut stream = MockStream::new(data);
    let result = parse_socks5_request(&mut stream).await;

    assert!(result.is_ok());
    let request = result.unwrap();
    if let Address::Domain(domain) = request.address {
        assert_eq!(domain.len(), 255);
    } else {
        panic!("Expected Domain address");
    }
}

#[tokio::test]
async fn test_socks5_request_zero_length_domain() {
    // Test with zero-length domain (will parse but domain will be empty string)
    let mut data = vec![
        SOCKS_VERSION,
        Command::Connect as u8,
        0x00,
        0x03, // Domain
        0,    // domain length = 0
    ];
    data.extend(&[0x00, 0x50]); // port 80

    let mut stream = MockStream::new(data);
    let result = parse_socks5_request(&mut stream).await;

    // Empty domain is technically valid in parsing, though may fail later in resolution
    assert!(result.is_ok());
    let request = result.unwrap();
    if let Address::Domain(domain) = request.address {
        assert_eq!(domain.len(), 0);
    } else {
        panic!("Expected Domain address");
    }
}

#[tokio::test]
async fn test_socks5_request_unsupported_command() {
    // Test with undefined command code (0xFF)
    let mut data = vec![
        SOCKS_VERSION,
        0xFF, // undefined command
        0x00,
        0x01, // IPv4
    ];
    data.extend(&[127, 0, 0, 1]);
    data.extend(&[0x00, 0x50]);

    let mut stream = MockStream::new(data);
    let result = parse_socks5_request(&mut stream).await;

    // Should fail with UnsupportedCommand error
    assert!(result.is_err());
    match result.unwrap_err() {
        RustSocksError::UnsupportedCommand(cmd) => {
            assert_eq!(cmd, 0xFF);
        }
        _ => panic!("Expected UnsupportedCommand error"),
    }
}

#[tokio::test]
async fn test_socks5_response_serialization() {
    // Test that we can serialize and send responses correctly
    let mut stream = MockStream::new(vec![]);

    let result = send_socks5_response(
        &mut stream,
        ReplyCode::Succeeded,
        Address::IPv4([127, 0, 0, 1]),
        1080,
    )
    .await;

    assert!(result.is_ok());
    assert!(!stream.write_buf.is_empty());

    // Verify the response format
    assert_eq!(stream.write_buf[0], SOCKS_VERSION);
    assert_eq!(stream.write_buf[1], ReplyCode::Succeeded as u8);
    assert_eq!(stream.write_buf[2], 0x00); // reserved
    assert_eq!(stream.write_buf[3], 0x01); // IPv4
}

#[tokio::test]
async fn test_udp_packet_fragmentation_not_supported() {
    // Test that fragmented UDP packets are rejected
    let mut data = vec![
        0x00, 0x00, // RSV
        0x01, // FRAG (non-zero = fragmentation)
        0x01, // IPv4
    ];
    data.extend(&[192, 168, 1, 1]); // IP
    data.extend(&[0x00, 0x50]); // port
    data.extend(b"test data");

    let result = parse_udp_packet(&data);

    assert!(result.is_err());
    match result.unwrap_err() {
        RustSocksError::Protocol(msg) => {
            assert!(msg.contains("UDP fragmentation not supported"));
        }
        _ => panic!("Expected Protocol error"),
    }
}

#[tokio::test]
async fn test_udp_packet_max_data_size() {
    // Test UDP packet with large data payload (up to 65507 bytes for UDP)
    let data_size = 60000; // Large but reasonable UDP payload
    let mut data = vec![
        0x00, 0x00, // RSV
        0x00, // FRAG
        0x01, // IPv4
    ];
    data.extend(&[192, 168, 1, 1]); // IP
    data.extend(&[0x00, 0x50]); // port
    data.extend(vec![0xAA; data_size]); // Large payload

    let result = parse_udp_packet(&data);

    assert!(result.is_ok());
    let packet = result.unwrap();
    assert_eq!(packet.data.len(), data_size);
}

#[tokio::test]
async fn test_udp_packet_empty_data() {
    // Test UDP packet with no data payload
    let mut data = vec![
        0x00, 0x00, // RSV
        0x00, // FRAG
        0x01, // IPv4
    ];
    data.extend(&[192, 168, 1, 1]); // IP
    data.extend(&[0x00, 0x50]); // port
                                // No data payload

    let result = parse_udp_packet(&data);

    assert!(result.is_ok());
    let packet = result.unwrap();
    assert_eq!(packet.data.len(), 0);
}

#[tokio::test]
async fn test_udp_packet_roundtrip() {
    // Test that we can serialize and deserialize UDP packets
    let original_packet = UdpPacket {
        header: UdpHeader {
            frag: 0,
            address: Address::Domain("example.com".to_string()),
            port: 8080,
        },
        data: b"Hello, UDP!".to_vec(),
    };

    let serialized = serialize_udp_packet(&original_packet);
    let deserialized = parse_udp_packet(&serialized).unwrap();

    assert_eq!(deserialized.header.port, original_packet.header.port);
    assert_eq!(deserialized.data, original_packet.data);

    if let (Address::Domain(orig), Address::Domain(deser)) = (
        &original_packet.header.address,
        &deserialized.header.address,
    ) {
        assert_eq!(orig, deser);
    } else {
        panic!("Address mismatch in roundtrip");
    }
}

#[tokio::test]
async fn test_concurrent_parse_operations() {
    // Test that multiple parse operations can run concurrently without issues
    use tokio::task::JoinSet;

    let mut set = JoinSet::new();

    // Spawn 100 concurrent parse operations
    for i in 0..100 {
        set.spawn(async move {
            let username = format!("user{}", i);
            let mut data = vec![0x01]; // version
            data.push(username.len() as u8); // username length
            data.extend(username.as_bytes());
            data.push(4); // password length
            data.extend(b"pass");

            let mut stream = MockStream::new(data);
            parse_userpass_auth(&mut stream).await
        });
    }

    // Wait for all to complete
    let mut success_count = 0;
    while let Some(result) = set.join_next().await {
        if result.is_ok() && result.unwrap().is_ok() {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 100);
}
