//! pack-vs-lock: Concurrency Strategy Benchmark
//!
//! Uses Criterion.rs for statistically rigorous performance measurement.
//! Compares 5 concurrency strategies under heavy multi-threaded contention.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread;

// ==========================================
// CONFIGURATION
// ==========================================

const THREAD_COUNTS: [usize; 6] = [1, 2, 4, 8, 16, 32];
const OPERATIONS_PER_THREAD: u64 = 100_000;

// ==========================================
// BENCHMARK VARIANTS
// ==========================================

/// Variant 1: Standard Mutex
fn bench_mutex(threads: usize) -> u64 {
    let counter = Arc::new(Mutex::new(0u64));
    let mut handles = vec![];

    for _ in 0..threads {
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..OPERATIONS_PER_THREAD {
                *counter.lock().unwrap() += 1;
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

 let x = *counter.lock().unwrap();
 x
}

/// Variant 2: RwLock (write-heavy)
fn bench_rwlock(threads: usize) -> u64 {
    let counter = Arc::new(RwLock::new(0u64));
    let mut handles = vec![];

    for _ in 0..threads {
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..OPERATIONS_PER_THREAD {
                *counter.write().unwrap() += 1;
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

   let x = *counter.read().unwrap();
   x
}

/// Variant 3: Spinlock with AtomicBool
fn bench_spinlock(threads: usize) -> u64 {
    let lock = Arc::new(AtomicBool::new(false));
    let counter = Arc::new(Mutex::new(0u64));
    let mut handles = vec![];

    for _ in 0..threads {
        let lock = Arc::clone(&lock);
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..OPERATIONS_PER_THREAD {
                while lock
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_err()
                {
                    thread::yield_now();
                }
                *counter.lock().unwrap() += 1;
                lock.store(false, Ordering::Release);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let x = *counter.lock().unwrap();
    x
}

/// Variant 4: Lock-Free Atomic (fetch_add)
fn bench_atomic(threads: usize) -> u64 {
    let counter = Arc::new(AtomicU64::new(0));
    let mut handles = vec![];

    for _ in 0..threads {
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..OPERATIONS_PER_THREAD {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    counter.load(Ordering::Relaxed)
}

/// Variant 5: Bit-Packed CAS (Two metrics in one AtomicU64)
fn bench_bitpacked(threads: usize) -> u64 {
    let counter = Arc::new(AtomicU64::new(0));
    let mut handles = vec![];

    for thread_id in 0..threads {
        let counter = Arc::clone(&counter);
        let is_security_thread = thread_id % 2 == 0;

        handles.push(thread::spawn(move || {
            for _ in 0..OPERATIONS_PER_THREAD {
                loop {
                    let current = counter.load(Ordering::Acquire);

                    // Unpack: spam (top 32) | total (bottom 32)
                    let total = (current & 0xFFFFFFFF) as u32;
                    let spam = (current >> 32) as u32;

                    // Mutate
                    let new_total = total + 1;
                    let new_spam = if is_security_thread { spam + 1 } else { spam };

                    // Pack
                    let target = ((new_spam as u64) << 32) | (new_total as u64);

                    // CAS loop
                    if counter
                        .compare_exchange_weak(current, target, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        break;
                    }
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Return total (bottom 32 bits)
    counter.load(Ordering::Relaxed) & 0xFFFFFFFF
}

// ==========================================
// CRITERION BENCHMARK GROUP
// ==========================================

fn bench_concurrency(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concurrency Strategies");

    // Configure for meaningful results
    group
        .sample_size(100) // 100 measurements per benchmark
        .measurement_time(std::time::Duration::from_secs(5))
        .warm_up_time(std::time::Duration::from_secs(3));

    for &threads in THREAD_COUNTS.iter() {
        let total_ops = threads as u64 * OPERATIONS_PER_THREAD;

        // 1. Mutex
        group.bench_with_input(
            BenchmarkId::new("Mutex", threads),
            &threads,
            |b, &threads| {
                b.iter(|| {
                    let result = bench_mutex(threads);
                    black_box(result);
                });
            },
        );

        // 2. RwLock
        group.bench_with_input(
            BenchmarkId::new("RwLock", threads),
            &threads,
            |b, &threads| {
                b.iter(|| {
                    let result = bench_rwlock(threads);
                    black_box(result);
                });
            },
        );

        // 3. Spinlock
        group.bench_with_input(
            BenchmarkId::new("Spinlock", threads),
            &threads,
            |b, &threads| {
                b.iter(|| {
                    let result = bench_spinlock(threads);
                    black_box(result);
                });
            },
        );

        // 4. Lock-Free Atomic
        group.bench_with_input(
            BenchmarkId::new("Atomic", threads),
            &threads,
            |b, &threads| {
                b.iter(|| {
                    let result = bench_atomic(threads);
                    black_box(result);
                });
            },
        );

        // 5. Bit-Packed CAS
        group.bench_with_input(
            BenchmarkId::new("BitPacked", threads),
            &threads,
            |b, &threads| {
                b.iter(|| {
                    let result = bench_bitpacked(threads);
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

// ==========================================
// VERIFICATION BENCHMARK (Optional)
// ==========================================

/// Verify all variants produce the correct result
#[test]
fn verify_correctness() {
    let threads = 4;
    let expected = threads as u64 * OPERATIONS_PER_THREAD;

    assert_eq!(bench_mutex(threads), expected);
    assert_eq!(bench_rwlock(threads), expected);
    assert_eq!(bench_spinlock(threads), expected);
    assert_eq!(bench_atomic(threads), expected);
    assert_eq!(bench_bitpacked(threads), expected);

    println!("✅ All variants produce correct results!");
}

// ==========================================
// CRITERION BOILERPLATE
// ==========================================

criterion_group!(benches, bench_concurrency);
criterion_main!(benches);
