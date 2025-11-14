/// Benchmark: Handshake Latency (flush behavior)
///
/// Measures the impact of flush() frequency on SOCKS5 handshake latency.
/// Note: This is mainly for measurement - changing flush behavior requires careful analysis
/// to avoid protocol synchronization issues.
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::io::{BufWriter, Write};

fn bench_multiple_small_writes_with_flush(c: &mut Criterion) {
    c.bench_function("small_writes_with_flush", |b| {
        b.iter(|| {
            let mut buffer = Vec::new();

            // Simulate SOCKS5 handshake with flush after each write
            {
                let mut writer = BufWriter::new(&mut buffer);

                // Server choice (2 bytes)
                writer.write_all(&[0x05, 0x00]).unwrap();
                writer.flush().unwrap(); // FLUSH 1

                // Auth response (2 bytes)
                writer.write_all(&[0x01, 0x00]).unwrap();
                writer.flush().unwrap(); // FLUSH 2

                // SOCKS5 response (10 bytes for IPv4)
                writer.write_all(&[0x05, 0x00, 0x00, 0x01]).unwrap();
                writer.write_all(&[127, 0, 0, 1]).unwrap(); // IP
                writer.write_all(&[0x04, 0x38]).unwrap(); // Port
                writer.flush().unwrap(); // FLUSH 3
            }

            black_box(buffer);
        });
    });
}

fn bench_batched_writes_single_flush(c: &mut Criterion) {
    c.bench_function("batched_writes_single_flush", |b| {
        b.iter(|| {
            let mut buffer = Vec::new();

            // Simulate SOCKS5 handshake with single flush at end
            {
                let mut writer = BufWriter::new(&mut buffer);

                // Server choice (2 bytes)
                writer.write_all(&[0x05, 0x00]).unwrap();

                // Auth response (2 bytes)
                writer.write_all(&[0x01, 0x00]).unwrap();

                // SOCKS5 response (10 bytes for IPv4)
                writer.write_all(&[0x05, 0x00, 0x00, 0x01]).unwrap();
                writer.write_all(&[127, 0, 0, 1]).unwrap(); // IP
                writer.write_all(&[0x04, 0x38]).unwrap(); // Port

                writer.flush().unwrap(); // SINGLE FLUSH
            }

            black_box(buffer);
        });
    });
}

fn bench_vectored_write_single_flush(c: &mut Criterion) {
    use std::io::IoSlice;

    c.bench_function("vectored_write_single_flush", |b| {
        b.iter(|| {
            let mut buffer = Vec::new();

            {
                let mut writer = BufWriter::new(&mut buffer);

                // Prepare all data upfront
                let server_choice = [0x05, 0x00];
                let auth_response = [0x01, 0x00];
                let socks_header = [0x05, 0x00, 0x00, 0x01];
                let ip = [127, 0, 0, 1];
                let port = [0x04, 0x38];

                // Write all at once using vectored I/O
                let slices = [
                    IoSlice::new(&server_choice),
                    IoSlice::new(&auth_response),
                    IoSlice::new(&socks_header),
                    IoSlice::new(&ip),
                    IoSlice::new(&port),
                ];
                writer.write_vectored(&slices).unwrap();
                writer.flush().unwrap();
            }

            black_box(buffer);
        });
    });
}

criterion_group!(
    benches,
    bench_multiple_small_writes_with_flush,
    bench_batched_writes_single_flush,
    bench_vectored_write_single_flush
);
criterion_main!(benches);
