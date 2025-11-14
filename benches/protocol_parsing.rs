/// Benchmark: Protocol Parsing Optimization
///
/// Compares Vec allocations vs SmallVec for protocol parsing operations.
/// This measures the performance impact of stack-allocated buffers in hot paths.
use criterion::{black_box, criterion_group, criterion_main, Criterion};

// Mock protocol parsing - measures allocation overhead
fn bench_vec_allocations(c: &mut Criterion) {
    c.bench_function("vec_small_allocations", |b| {
        b.iter(|| {
            // Simulate multiple small allocations (typical SOCKS5 handshake)
            let mut buffers = Vec::new();

            // Methods buffer (3 bytes)
            let methods = vec![0u8; 3];
            buffers.push(methods);

            // Username (16 bytes)
            let username = vec![0u8; 16];
            buffers.push(username);

            // Password (16 bytes)
            let password = vec![0u8; 16];
            buffers.push(password);

            // Domain (32 bytes)
            let domain = vec![0u8; 32];
            buffers.push(domain);

            // Response buffer (22 bytes)
            let response = vec![0u8; 22];
            buffers.push(response);

            black_box(buffers);
        });
    });
}

fn bench_smallvec_allocations(c: &mut Criterion) {
    use smallvec::SmallVec;

    c.bench_function("smallvec_small_allocations", |b| {
        b.iter(|| {
            // Same allocations but using SmallVec (stack-allocated)
            let mut buffers = Vec::new();

            // Methods buffer (3 bytes) - fits in stack
            let methods = SmallVec::<[u8; 8]>::from_elem(0, 3);
            buffers.push(methods.to_vec());

            // Username (16 bytes) - fits in stack
            let username = SmallVec::<[u8; 64]>::from_elem(0, 16);
            buffers.push(username.to_vec());

            // Password (16 bytes) - fits in stack
            let password = SmallVec::<[u8; 64]>::from_elem(0, 16);
            buffers.push(password.to_vec());

            // Domain (32 bytes) - fits in stack
            let domain = SmallVec::<[u8; 128]>::from_elem(0, 32);
            buffers.push(domain.to_vec());

            // Response buffer (22 bytes) - fits in stack
            let response = SmallVec::<[u8; 256]>::new();
            buffers.push(response.to_vec());

            black_box(buffers);
        });
    });
}

fn bench_vec_capacity_prealloc(c: &mut Criterion) {
    c.bench_function("vec_with_capacity", |b| {
        b.iter(|| {
            // Serialize UDP packet with pre-allocated capacity
            let header_size = 4 + 4 + 2; // RSV + FRAG + ATYP + IPv4 + PORT
            let data_size = 512;
            let mut buf = Vec::with_capacity(header_size + data_size);

            buf.extend_from_slice(&[0x00, 0x00]); // RSV
            buf.push(0x00); // FRAG
            buf.push(0x01); // IPv4
            buf.extend_from_slice(&[127, 0, 0, 1]); // IP
            buf.extend_from_slice(&[0x00, 0x50]); // Port 80
            buf.extend_from_slice(&vec![0u8; data_size]); // Data

            black_box(buf);
        });
    });
}

fn bench_vec_no_prealloc(c: &mut Criterion) {
    c.bench_function("vec_without_capacity", |b| {
        b.iter(|| {
            // Serialize UDP packet WITHOUT pre-allocated capacity (old approach)
            let data_size = 512;
            let mut buf = Vec::new(); // Will reallocate multiple times

            buf.extend_from_slice(&[0x00, 0x00]); // RSV
            buf.push(0x00); // FRAG
            buf.push(0x01); // IPv4
            buf.extend_from_slice(&[127, 0, 0, 1]); // IP
            buf.extend_from_slice(&[0x00, 0x50]); // Port 80
            buf.extend_from_slice(&vec![0u8; data_size]); // Data

            black_box(buf);
        });
    });
}

// Realistic SOCKS5 handshake simulation
fn bench_full_handshake_vec(c: &mut Criterion) {
    c.bench_function("handshake_vec", |b| {
        b.iter(|| {
            // Client greeting: version + nmethods + methods
            let greeting = vec![0x05, 0x02, 0x00, 0x02];

            // Server choice: version + method
            let choice = vec![0x05, 0x00];

            // Username/password auth
            let username = "alice".as_bytes();
            let password = "secret123".as_bytes();
            let mut auth_buf = vec![0x01]; // version
            auth_buf.push(username.len() as u8);
            auth_buf.extend_from_slice(username);
            auth_buf.push(password.len() as u8);
            auth_buf.extend_from_slice(password);

            // Auth response
            let auth_response = vec![0x01, 0x00];

            // SOCKS5 request (domain)
            let domain = "example.com".as_bytes();
            let mut request = vec![0x05, 0x01, 0x00, 0x03]; // version, connect, reserved, domain type
            request.push(domain.len() as u8);
            request.extend_from_slice(domain);
            request.extend_from_slice(&[0x01, 0xBB]); // Port 443

            // SOCKS5 response
            let mut response = vec![0x05, 0x00, 0x00]; // version, success, reserved
            response.push(0x01); // IPv4
            response.extend_from_slice(&[127, 0, 0, 1]); // IP
            response.extend_from_slice(&[0x04, 0x38]); // Port 1080

            black_box((greeting, choice, auth_buf, auth_response, request, response));
        });
    });
}

fn bench_full_handshake_smallvec(c: &mut Criterion) {
    use smallvec::SmallVec;

    c.bench_function("handshake_smallvec", |b| {
        b.iter(|| {
            // Client greeting: version + nmethods + methods
            let greeting = SmallVec::<[u8; 32]>::from_slice(&[0x05, 0x02, 0x00, 0x02]);

            // Server choice: version + method
            let choice = SmallVec::<[u8; 2]>::from_slice(&[0x05, 0x00]);

            // Username/password auth
            let username = "alice".as_bytes();
            let password = "secret123".as_bytes();
            let mut auth_buf = SmallVec::<[u8; 128]>::new();
            auth_buf.push(0x01); // version
            auth_buf.push(username.len() as u8);
            auth_buf.extend_from_slice(username);
            auth_buf.push(password.len() as u8);
            auth_buf.extend_from_slice(password);

            // Auth response
            let auth_response = SmallVec::<[u8; 2]>::from_slice(&[0x01, 0x00]);

            // SOCKS5 request (domain)
            let domain = "example.com".as_bytes();
            let mut request = SmallVec::<[u8; 256]>::new();
            request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03]);
            request.push(domain.len() as u8);
            request.extend_from_slice(domain);
            request.extend_from_slice(&[0x01, 0xBB]); // Port 443

            // SOCKS5 response
            let mut response = SmallVec::<[u8; 256]>::new();
            response.extend_from_slice(&[0x05, 0x00, 0x00]);
            response.push(0x01); // IPv4
            response.extend_from_slice(&[127, 0, 0, 1]); // IP
            response.extend_from_slice(&[0x04, 0x38]); // Port 1080

            black_box((greeting, choice, auth_buf, auth_response, request, response));
        });
    });
}

criterion_group!(
    benches,
    bench_vec_allocations,
    bench_smallvec_allocations,
    bench_vec_capacity_prealloc,
    bench_vec_no_prealloc,
    bench_full_handshake_vec,
    bench_full_handshake_smallvec
);
criterion_main!(benches);
