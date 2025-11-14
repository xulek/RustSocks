/// Benchmark: UDP Packet Serialization Optimization
///
/// Compares Vec::new() vs Vec::with_capacity() for UDP packet serialization.
/// Pre-allocating capacity avoids multiple reallocations during packet construction.
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

fn bench_udp_serialize_no_capacity(c: &mut Criterion) {
    c.bench_function("udp_serialize_no_capacity", |b| {
        b.iter(|| {
            // OLD: No pre-allocation (multiple reallocations)
            let mut buf = Vec::new();

            // RSV (2 bytes)
            buf.extend_from_slice(&[0x00, 0x00]);

            // FRAG (1 byte)
            buf.push(0x00);

            // ATYP + Address (IPv4 = 5 bytes)
            buf.push(0x01);
            buf.extend_from_slice(&[127, 0, 0, 1]);

            // Port (2 bytes)
            buf.extend_from_slice(&[0x00, 0x50]);

            // Data (512 bytes)
            let data = vec![0xAB; 512];
            buf.extend_from_slice(&data);

            black_box(buf);
        });
    });
}

fn bench_udp_serialize_with_capacity(c: &mut Criterion) {
    c.bench_function("udp_serialize_with_capacity", |b| {
        b.iter(|| {
            // NEW: Pre-allocate exact capacity
            let data_len = 512;
            let header_size = 4 + 4 + 2; // RSV + FRAG + ATYP + IPv4 + PORT
            let mut buf = Vec::with_capacity(header_size + data_len);

            // RSV (2 bytes)
            buf.extend_from_slice(&[0x00, 0x00]);

            // FRAG (1 byte)
            buf.push(0x00);

            // ATYP + Address (IPv4 = 5 bytes)
            buf.push(0x01);
            buf.extend_from_slice(&[127, 0, 0, 1]);

            // Port (2 bytes)
            buf.extend_from_slice(&[0x00, 0x50]);

            // Data (512 bytes)
            let data = vec![0xAB; 512];
            buf.extend_from_slice(&data);

            black_box(buf);
        });
    });
}

fn bench_udp_serialize_domain_no_capacity(c: &mut Criterion) {
    c.bench_function("udp_serialize_domain_no_capacity", |b| {
        b.iter(|| {
            let domain = "example.com";
            let mut buf = Vec::new();

            // RSV + FRAG
            buf.extend_from_slice(&[0x00, 0x00, 0x00]);

            // ATYP + Domain
            buf.push(0x03);
            buf.push(domain.len() as u8);
            buf.extend_from_slice(domain.as_bytes());

            // Port
            buf.extend_from_slice(&[0x01, 0xBB]);

            // Data (1024 bytes)
            let data = vec![0xCD; 1024];
            buf.extend_from_slice(&data);

            black_box(buf);
        });
    });
}

fn bench_udp_serialize_domain_with_capacity(c: &mut Criterion) {
    c.bench_function("udp_serialize_domain_with_capacity", |b| {
        b.iter(|| {
            let domain = "example.com";
            let data_len = 1024;
            let header_size = 4 + 1 + domain.len() + 2; // RSV + FRAG + ATYP + LEN + DOMAIN + PORT
            let mut buf = Vec::with_capacity(header_size + data_len);

            // RSV + FRAG
            buf.extend_from_slice(&[0x00, 0x00, 0x00]);

            // ATYP + Domain
            buf.push(0x03);
            buf.push(domain.len() as u8);
            buf.extend_from_slice(domain.as_bytes());

            // Port
            buf.extend_from_slice(&[0x01, 0xBB]);

            // Data (1024 bytes)
            let data = vec![0xCD; 1024];
            buf.extend_from_slice(&data);

            black_box(buf);
        });
    });
}

fn bench_various_packet_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("packet_sizes");

    for size in [64, 256, 512, 1024, 4096].iter() {
        group.bench_with_input(BenchmarkId::new("no_capacity", size), size, |b, &size| {
            b.iter(|| {
                let mut buf = Vec::new();
                buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                buf.extend_from_slice(&[127, 0, 0, 1]);
                buf.extend_from_slice(&[0x00, 0x50]);
                let data = vec![0xFF; size];
                buf.extend_from_slice(&data);
                black_box(buf);
            });
        });

        group.bench_with_input(BenchmarkId::new("with_capacity", size), size, |b, &size| {
            b.iter(|| {
                let header_size = 10;
                let mut buf = Vec::with_capacity(header_size + size);
                buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                buf.extend_from_slice(&[127, 0, 0, 1]);
                buf.extend_from_slice(&[0x00, 0x50]);
                let data = vec![0xFF; size];
                buf.extend_from_slice(&data);
                black_box(buf);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_udp_serialize_no_capacity,
    bench_udp_serialize_with_capacity,
    bench_udp_serialize_domain_no_capacity,
    bench_udp_serialize_domain_with_capacity,
    bench_various_packet_sizes
);
criterion_main!(benches);
