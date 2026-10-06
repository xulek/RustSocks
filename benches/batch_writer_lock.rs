/// Benchmark: Batch Writer Lock Optimization
///
/// Compares Mutex<Option<Arc<T>>> vs OnceLock<Arc<T>> for read-heavy workloads.
/// This simulates the session manager accessing batch writer on every session creation.
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::{Arc, Barrier, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

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

/// Contended reads: threads are spawned once per measurement and released by a
/// barrier, so only the read loop is timed rather than thread creation.
fn bench_contention(c: &mut Criterion) {
    const READS_PER_THREAD: usize = 10_000;
    let mut group = c.benchmark_group("contention");

    for thread_count in [2usize, 4, 8].iter().copied() {
        group.bench_with_input(
            BenchmarkId::new("mutex", thread_count),
            &thread_count,
            |b, &threads| {
                let accessor = Arc::new(MutexBasedAccess::new());
                b.iter_custom(|iters| {
                    timed_contention(threads, iters, READS_PER_THREAD, &accessor, |a| {
                        a.get_writer()
                    })
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("oncelock", thread_count),
            &thread_count,
            |b, &threads| {
                let accessor = Arc::new(OnceLockBasedAccess::new());
                b.iter_custom(|iters| {
                    timed_contention(threads, iters, READS_PER_THREAD, &accessor, |a| {
                        a.get_writer()
                    })
                });
            },
        );
    }

    group.finish();
}

fn timed_contention<T, F>(
    threads: usize,
    iters: u64,
    reads: usize,
    accessor: &Arc<T>,
    read: F,
) -> Duration
where
    T: Send + Sync + 'static,
    F: Fn(&T) -> Option<Arc<MockBatchWriter>> + Copy + Send + 'static,
{
    let barrier = Arc::new(Barrier::new(threads + 1));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let accessor = Arc::clone(accessor);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for _ in 0..iters {
                    for _ in 0..reads {
                        black_box(read(&accessor));
                    }
                }
            })
        })
        .collect();

    barrier.wait();
    let start = Instant::now();
    for handle in handles {
        handle.join().unwrap();
    }
    start.elapsed()
}

criterion_group!(
    benches,
    bench_mutex_single_thread,
    bench_oncelock_single_thread,
    bench_contention
);
criterion_main!(benches);
