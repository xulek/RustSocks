/// Benchmark: Batch Writer Lock Optimization
///
/// Compares Mutex<Option<Arc<T>>> vs OnceLock<Arc<T>> for read-heavy workloads.
/// This simulates the session manager accessing batch writer on every session creation.
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

// Mock BatchWriter for benchmarking
#[derive(Clone)]
#[allow(dead_code)]
struct MockBatchWriter {
    id: u64,
}

// OLD: Mutex-based approach (before optimization)
struct MutexBasedAccess {
    writer: Arc<Mutex<Option<Arc<MockBatchWriter>>>>,
}

impl MutexBasedAccess {
    fn new() -> Self {
        let writer = Arc::new(MockBatchWriter { id: 42 });
        Self {
            writer: Arc::new(Mutex::new(Some(writer))),
        }
    }

    fn get_writer(&self) -> Option<Arc<MockBatchWriter>> {
        self.writer.lock().unwrap().clone()
    }
}

// NEW: OnceLock-based approach (after optimization)
struct OnceLockBasedAccess {
    writer: OnceLock<Arc<MockBatchWriter>>,
}

impl OnceLockBasedAccess {
    fn new() -> Self {
        let instance = Self {
            writer: OnceLock::new(),
        };
        let writer = Arc::new(MockBatchWriter { id: 42 });
        let _ = instance.writer.set(writer);
        instance
    }

    fn get_writer(&self) -> Option<Arc<MockBatchWriter>> {
        self.writer.get().cloned()
    }
}

fn bench_mutex_single_thread(c: &mut Criterion) {
    let accessor = MutexBasedAccess::new();

    c.bench_function("mutex_single_thread", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                let writer = accessor.get_writer();
                black_box(writer);
            }
        });
    });
}

fn bench_oncelock_single_thread(c: &mut Criterion) {
    let accessor = OnceLockBasedAccess::new();

    c.bench_function("oncelock_single_thread", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                let writer = accessor.get_writer();
                black_box(writer);
            }
        });
    });
}

fn bench_mutex_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("contention");

    for thread_count in [2, 4, 8, 16].iter() {
        group.bench_with_input(
            BenchmarkId::new("mutex", thread_count),
            thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let accessor = Arc::new(MutexBasedAccess::new());
                    let mut handles = vec![];

                    for _ in 0..thread_count {
                        let accessor = accessor.clone();
                        let handle = thread::spawn(move || {
                            for _ in 0..100 {
                                let writer = accessor.get_writer();
                                black_box(writer);
                            }
                        });
                        handles.push(handle);
                    }

                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_oncelock_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("contention");

    for thread_count in [2, 4, 8, 16].iter() {
        group.bench_with_input(
            BenchmarkId::new("oncelock", thread_count),
            thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    // Share OnceLockBasedAccess across threads via Arc
                    let accessor = Arc::new(OnceLockBasedAccess::new());
                    let mut handles = vec![];

                    for _ in 0..thread_count {
                        let accessor = accessor.clone();
                        let handle = thread::spawn(move || {
                            for _ in 0..100 {
                                let writer = accessor.get_writer();
                                black_box(writer);
                            }
                        });
                        handles.push(handle);
                    }

                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_mutex_single_thread,
    bench_oncelock_single_thread,
    bench_mutex_contention,
    bench_oncelock_contention
);
criterion_main!(benches);
