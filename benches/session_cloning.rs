use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rustsocks::session::types::{ConnectionInfo, Protocol, Session};
use std::net::IpAddr;

fn build_session() -> Session {
    let conn = ConnectionInfo {
        source_ip: IpAddr::from([127, 0, 0, 1]),
        source_port: 50000,
        dest_ip: "example.com".to_string(),
        dest_port: 443,
        protocol: Protocol::Tcp,
    };

    Session::new("bench-user", conn, "allow", Some("bench-rule".to_string()))
}

fn bench_session_clone(c: &mut Criterion) {
    let session = build_session();

    c.bench_function("session_clone", |b| {
        b.iter(|| {
            let cloned = session.clone();
            black_box(cloned);
        });
    });
}

criterion_group!(benches, bench_session_clone);
criterion_main!(benches);
