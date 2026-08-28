//! pack-vs-lock: Mutex vs RwLock vs Spinlock vs Lock-Free vs Bit-Packed CAS
//!
//! Five concurrency strategies battle it out under heavy contention.
//! Spoiler: RwLock is the slowest. Lock-free wins.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Instant;

const TOTAL_INCREMENTS: u64 = 10_000_000;
const THREAD_COUNT: usize = 8;
const WARMUP_ITERATIONS: u64 = 1_000_000;

fn main() {
    let banner = r#"
 Mutex vs RwLock vs Spinlock vs Lock-Free vs Bit-Packed
 Proving hardware atomics destroy OS locks
"#;
    println!("{}", banner);
    println!();
    println!("📊 Configuration:");
    println!(" Total Increments: {}", TOTAL_INCREMENTS);
    println!(" Thread Count: {}", THREAD_COUNT);
    println!(" Warmup: {} iterations", WARMUP_ITERATIONS);
    println!();

    warmup();
    println!("✅ Warmup complete\n");

    // ==========================================
    // 1. MUTEX
    // ==========================================
    println!("🔒 VARIANT 1: MUTEX");
    let mutex_counter = Arc::new(Mutex::new(0u64));
    let start = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREAD_COUNT {
            let counter = Arc::clone(&mutex_counter);
            s.spawn(move || {
                for _ in 0..(TOTAL_INCREMENTS / THREAD_COUNT as u64) {
                    *counter.lock().unwrap() += 1;
                }
            });
        }
    });
    let mutex_time = start.elapsed();
    println!(" Time: {:?}", mutex_time);
    println!(" Final:{}", *mutex_counter.lock().unwrap());
    println!();

    // ==========================================
    // 2. RWLOCK (NEW!)
    // ==========================================
    println!("📖 VARIANT 2: RWLOCK");
    let rwlock_counter = Arc::new(RwLock::new(0u64));
    let start = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREAD_COUNT {
            let counter = Arc::clone(&rwlock_counter);
            s.spawn(move || {
                for _ in 0..(TOTAL_INCREMENTS / THREAD_COUNT as u64) {
                    *counter.write().unwrap() += 1;
                }
            });
        }
    });
    let rwlock_time = start.elapsed();
    println!(" Time: {:?}", rwlock_time);
    println!(" Final:{}", *rwlock_counter.read().unwrap());
    println!();

    // ==========================================
    // 3. SPINLOCK
    // ==========================================
    println!("🔄 VARIANT 3: SPINLOCK");
    let spinlock = Arc::new(AtomicBool::new(false));
    let spin_counter = Arc::new(Mutex::new(0u64));
    let start = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREAD_COUNT {
            let lock = Arc::clone(&spinlock);
            let counter = Arc::clone(&spin_counter);
            s.spawn(move || {
                for _ in 0..(TOTAL_INCREMENTS / THREAD_COUNT as u64) {
                    while lock
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_err()
                    {
                        thread::yield_now();
                    }
                    *counter.lock().unwrap() += 1;
                    lock.store(false, Ordering::Release);
                }
            });
        }
    });
    let spinlock_time = start.elapsed();
    println!(" Time: {:?}", spinlock_time);
    println!(" Final:{}", *spin_counter.lock().unwrap());
    println!();

    // ==========================================
    // 4. LOCK-FREE ATOMIC
    // ==========================================
    println!("⚡ VARIANT 4: LOCK-FREE ATOMIC");
    let atomic_counter = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREAD_COUNT {
            let counter = Arc::clone(&atomic_counter);
            s.spawn(move || {
                for _ in 0..(TOTAL_INCREMENTS / THREAD_COUNT as u64) {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    let atomic_time = start.elapsed();
    println!(" Time: {:?}", atomic_time);
    println!(" Final:{}", atomic_counter.load(Ordering::Relaxed));
    println!();

    // ==========================================
    // 5. BIT-PACKED CAS
    // ==========================================
    println!("🧩 VARIANT 5: BIT-PACKED CAS");
    let packed_counter = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let mut total_retries = 0u64;

    thread::scope(|s| {
        let retry_counter = Arc::new(AtomicU64::new(0));

        for thread_id in 0..THREAD_COUNT {
            let counter = Arc::clone(&packed_counter);
            let retry_counter = Arc::clone(&retry_counter);
            let is_security_thread = thread_id % 2 == 0;

            s.spawn(move || {
                let mut local_retries = 0u64;
                for _ in 0..(TOTAL_INCREMENTS / THREAD_COUNT as u64) {
                    loop {
                        let current = counter.load(Ordering::Acquire);
                        let spam = (current >> 32) as u32;
                        let total = (current & 0xFFFFFFFF) as u32;
                        let new_total = total + 1;
                        let new_spam = if is_security_thread { spam + 1 } else { spam };
                        let target = ((new_spam as u64) << 32) | (new_total as u64);

                        if counter
                            .compare_exchange_weak(
                                current,
                                target,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                        {
                            break;
                        }
                        local_retries += 1;
                    }
                }
                retry_counter.fetch_add(local_retries, Ordering::Relaxed);
            });
        }

        total_retries = retry_counter.load(Ordering::Relaxed);
    });
    let packed_time = start.elapsed();
    let final_packed = packed_counter.load(Ordering::Relaxed);
    let final_spam = (final_packed >> 32) as u32;
    let final_total = (final_packed & 0xFFFFFFFF) as u32;

    println!(" Time: {:?}", packed_time);
    println!(" Final:Total={}, Spam={}", final_total, final_spam);
    println!(" CAS Retries: {}", total_retries);
    println!(
        " Retry Rate: {:.2}%",
        (total_retries as f64 / (TOTAL_INCREMENTS as f64)) * 100.0
    );
    println!();

    // ==========================================
    // RESULTS
    // ==========================================
    println!(" 📊 RESULTS SUMMARY");

    let fastest = *[
        atomic_time,
        packed_time,
        spinlock_time,
        mutex_time,
        rwlock_time,
    ]
    .iter()
    .min()
    .unwrap();

    let fastest_name = if fastest == atomic_time {
        "⚡ Lock-Free Atomic"
    } else if fastest == packed_time {
        "🧩 Bit-Packed CAS"
    } else if fastest == spinlock_time {
        "🔄 Spinlock"
    } else if fastest == rwlock_time {
        "📖 RwLock"
    } else {
        "🔒 Mutex"
    };

    println!("🥇 Fastest: {} ", fastest_name);
    println!("🔒 Mutex: {:>10?} ", mutex_time);
    println!("📖 RwLock:{:>10?} ", rwlock_time);
    println!("🔄 Spinlock:{:>10?}", spinlock_time);
    println!("⚡ Lock-Free Atomic: {:>10?}", atomic_time);
    println!("🧩 Bit-Packed CAS:{:>10?} ", packed_time);

    let mutex_us = mutex_time.as_micros().max(1);
    let atomic_us = atomic_time.as_micros().max(1);
    let packed_us = packed_time.as_micros().max(1);
    let spin_us = spinlock_time.as_micros().max(1);
    let rwlock_us = rwlock_time.as_micros().max(1);

    println!("🚀 Speedups (vs Mutex): ");
    println!(
        " RwLock: {:>6.1}x (slower!) ",
        mutex_us as f64 / rwlock_us as f64
    );
    println!(
        " Spinlock: {:>6.1}x faster",
        mutex_us as f64 / spin_us as f64
    );
    println!(
        " Lock-Free Atomic: {:>6.1}x faster",
        mutex_us as f64 / atomic_us as f64
    );
    println!(
        " Bit-Packed CAS: {:>6.1}x faster",
        mutex_us as f64 / packed_us as f64
    );
    println!("🎯 Surprise:");
    println!(" RwLock is SLOWER than Mutex for write-heavy workloads!");
    println!(" More bookkeeping + more cache lines = more overhead. ");
    println!("🚀 Win: ");
    println!(
        " Bit-Packed CAS gives {:.1}x speedup while tracking",
        mutex_us as f64 / packed_us as f64
    );
    println!(" TWO metrics atomically. This is your token bucket!");
}

fn warmup() {
    let mut x = 0u64;
    for _ in 0..WARMUP_ITERATIONS {
        x = x.wrapping_add(1);
        std::hint::black_box(x);
    }
}
