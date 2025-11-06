# Protocol Implementation

This document covers the detailed implementation of SOCKS5 protocol extensions: UDP ASSOCIATE, BIND command, and SOCKS over TLS.

## UDP ASSOCIATE Command

**Implementation Status**: ✅ Complete

The UDP ASSOCIATE command enables UDP traffic relaying through the SOCKS5 proxy.

### How It Works

1. **TCP Control Connection**: Client sends UDP ASSOCIATE request over TCP
2. **UDP Relay Binding**: Server binds a UDP socket and returns the address/port to client
3. **UDP Packet Format**: All UDP packets use SOCKS5 UDP packet format (RFC 1928):
   ```
   +----+------+------+----------+----------+----------+
   |RSV | FRAG | ATYP | DST.ADDR | DST.PORT |   DATA   |
   +----+------+------+----------+----------+----------+
   | 2  |  1   |  1   | Variable |    2     | Variable |
   +----+------+------+----------+----------+----------+
   ```
4. **Bidirectional Relay**: Server forwards packets between client and destination
5. **Session Lifetime**: UDP session remains active while TCP control connection is open
6. **Timeout**: 120-second idle timeout (no packets in either direction)

### Key Components

- **`protocol/types.rs`**: `UdpHeader`, `UdpPacket` structures
- **`protocol/parser.rs`**: `parse_udp_packet()`, `serialize_udp_packet()` functions
- **`server/udp.rs`**: UDP relay implementation
  - `UdpSessionMap`: Tracks client-to-destination mappings
  - `handle_udp_associate()`: Main UDP relay handler
  - `run_udp_relay()`: Relay loop with timeout
  - `handle_client_packet()`: Forward client → destination
  - `handle_destination_packet()`: Forward destination → client
- **`server/handler.rs`**: Integration with main handler flow

### Features

- ✅ Full SOCKS5 UDP packet encapsulation
- ✅ Bidirectional UDP forwarding
- ✅ ACL enforcement (TCP/UDP protocol filtering)
- ✅ Session tracking and traffic metrics
- ✅ IPv4/IPv6/domain name support
- ✅ Automatic cleanup on TCP disconnect
- ✅ 120-second idle timeout
- ❌ UDP fragmentation not supported (FRAG must be 0)

### Testing

```bash
# Run UDP tests
cargo test --all-features udp

# Integration tests include:
# - Basic UDP ASSOCIATE flow
# - ACL allow/block for UDP
# - Session tracking
```

## BIND Command

**Implementation Status**: ✅ Complete

The BIND command enables reverse connections through the SOCKS5 proxy, allowing incoming connections to reach the client.

### How It Works

1. **BIND Request**: Client sends BIND command specifying destination address and port
2. **Listener Binding**: Server binds a TCP listener on an ephemeral port (0)
3. **First Response**: Server sends first SOCKS5 response with the bind address/port
4. **Wait for Connection**: Server waits up to 300 seconds (RFC 1928) for incoming connection
5. **Second Response**: Server sends second response with the connecting peer's address/port
6. **Data Proxying**: Server proxies data bidirectionally between client and incoming connection
7. **Session Cleanup**: Session closes when connection ends

### Key Components

- **`server/bind.rs`**: BIND command implementation
  - `handle_bind()`: Main BIND handler
  - `send_bind_response()`: Send SOCKS5 BIND responses
  - `BIND_ACCEPT_TIMEOUT`: 300-second timeout per RFC 1928
- **`server/handler.rs`**: Integration with main handler flow (Command::Bind match)

### Features

- ✅ RFC 1928 compliant (300-second timeout)
- ✅ Two-response protocol (bind address, then peer address)
- ✅ ACL enforcement for incoming connections
- ✅ Session tracking and traffic metrics
- ✅ IPv4/IPv6 address support
- ✅ Proper timeout handling with error responses
- ✅ Bidirectional data proxying

### BIND Response Format

```
+----+-----+-------+------+----------+----------+
|VER | REP |  RSV  | ATYP | BND.ADDR | BND.PORT |
+----+-----+-------+------+----------+----------+
| 1  |  1  |   1   |  1   | Variable |    2     |
+----+-----+-------+------+----------+----------+
```

### Testing

```bash
# Run BIND tests
cargo test --all-features bind

# Integration tests include:
# - Basic BIND handshake
# - BIND with incoming connection acceptance
# - ACL allow/block for BIND
# - Session tracking
```

## SOCKS over TLS

**Implementation Status**: ✅ Complete

RustSocks supports full TLS encryption for SOCKS5 connections, including mutual TLS (mTLS) with client certificate authentication.

### Features

- ✅ Full TLS 1.2 and TLS 1.3 support
- ✅ Server certificate configuration
- ✅ Mutual TLS (mTLS) with client authentication
- ✅ Configurable protocol versions
- ✅ Integration with all authentication methods
- ✅ Session tracking with encrypted connections

### Key Components

- **`src/server/listener.rs`**: `create_tls_acceptor()` - TLS initialization
  - Certificate and key loading
  - Protocol version configuration
  - Client CA path (for mTLS)
- **`src/config/mod.rs`**: `TlsSettings` - Configuration struct
- **Integration tests**: `tests/tls_support.rs`
  - Basic SOCKS5 over TLS
  - Mutual TLS with client certificates

### Configuration

```toml
[server.tls]
enabled = true
certificate_path = "/etc/rustsocks/server.crt"
private_key_path = "/etc/rustsocks/server.key"
min_protocol_version = "TLS13"  # or "TLS12"

# For mutual TLS (client authentication):
require_client_auth = true
client_ca_path = "/etc/rustsocks/clients-ca.crt"
```

### Testing

```bash
# Run TLS integration tests
cargo test --all-features tls_support

# Test with mTLS (requires client cert)
cargo test --all-features socks5_connect_with_mutual_tls
```

### Security Benefits

- **Encryption**: All SOCKS5 handshake and data traffic encrypted
- **No plaintext credentials**: Even with username/password auth, credentials are transmitted over TLS
- **mTLS support**: Client certificate validation for additional security
- **Protocol enforcement**: Can require TLS 1.3 minimum for maximum security

### Certificate Generation

```bash
# Generate self-signed certificate (for testing)
openssl req -x509 -newkey rsa:4096 \
  -keyout server.key -out server.crt \
  -days 365 -nodes

# Production: Use certificates from trusted CA
# Place in /etc/rustsocks/ and set permissions
sudo chmod 600 /etc/rustsocks/server.key
sudo chmod 644 /etc/rustsocks/server.crt
```

### Typical Deployment

For production deployments:
1. Obtain certificates from a trusted CA (Let's Encrypt, commercial CA)
2. Place certificates in secure location (`/etc/rustsocks/`)
3. Set restrictive file permissions (600 for private key)
4. Configure TLS in `rustsocks.toml`
5. Optionally enable mTLS for client authentication
6. Test with `openssl s_client` before deploying clients

## Related Documentation

- [Architecture Overview](architecture.md)
- [Session Management](session-management.md)
- [Testing Guide](../guides/testing.md)
