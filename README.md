# 🔬 pack-vs-lock

> **Mutex vs RwLock vs Spinlock vs Lock-Free vs Bit-Packed CAS**

A production-grade benchmarking suite comparing traditional locking, spinning, lock-free atomics, and bit-packed CAS under concurrent workloads.

![Rust](https://img.shields.io/badge/rust-1.97.1%2B-orange.svg)
![License](https://img.shields.io/badge/License-MIT-yellow.svg)
![Benchmarked with Criterion](https://img.shields.io/badge/benchmarked%20with-criterion-5e5e5e.svg)

---

## 📖 The Story Behind This Project

As systems scale to handle millions of requests per second, the **single most important bottleneck** can become how threads coordinate access to shared state.

Most engineers default to using `Mutex` or `RwLock` — it's safe, familiar, and works. But **safe isn't necessarily fast**, and familiar abstractions can hide significant synchronization costs.

This benchmark explores five different approaches to concurrent state updates and answers three questions:

1. **How much slower is a Mutex under real contention?**
2. **Is RwLock faster than Mutex for write-heavy workloads?**
3. **Can bit-packing let you update multiple metrics atomically without locks?**

---

## 🎯 The Five Contenders

We pit five concurrency strategies against each other in a multi-threaded increment benchmark.

### 🔒 Variant 1: The Monolithic Mutex

*The industry default.*

- Standard `std::sync::Mutex` wrapping a single integer
- Every increment requires acquiring and releasing the lock
- Threads may block under contention

**Result:** `432 ms` for 10M increments

---

### 📖 Variant 2: RwLock

*The "smart" choice?*

- `std::sync::RwLock` with write locks for every operation
- Multiple readers or one writer
- More bookkeeping than a simple `Mutex`

**Result:** `671 ms` — **slower than Mutex**

---

### 🔄 Variant 3: Spinlock with AtomicBool

*The naive optimization.*

- Busy-waits on an `AtomicBool` flag
- Avoids blocking on an OS mutex
- Threads continue consuming CPU while waiting
- Still suffers from all-or-nothing locking

**Result:** `372 ms` — faster than Mutex, but still expensive

---

### ⚡ Variant 4: Lock-Free Atomic

*The hardware-accelerated solution.*

- Single `AtomicU64`
- Uses `fetch_add(1, Ordering::Relaxed)`
- Maps to hardware atomic operations such as `LOCK XADD` on x86
- No mutex acquisition; the atomic operation itself does not take a blocking lock

**Result:** `160 ms` — **2.7× faster than Mutex**

---

### 🧩 Variant 5: Bit-Packed CAS

*The token-bucket architecture.*

Two counters are packed into a single `AtomicU64`:

- **Bottom 32 bits:** Total requests
- **Top 32 bits:** Spam/flagged requests
- CAS loop updates both metrics atomically
- Uses bitwise operations such as `>>`, `&`, and `|`

**Result:** `451 ms` — approximately the same order of magnitude as Mutex, while tracking **two metrics atomically**

---

## 📊 Results

The benchmark was run on an 8-core CPU with 8 threads performing 10 million increments.

```text
╔═══════════════════════════════════════════════════════════════════╗
║                       📊 RESULTS SUMMARY                          ║
╠═══════════════════════════════════════════════════════════════════╣
║  🥇 Fastest:         ⚡ Lock-Free Atomic                         ║
║                                                                   ║
║  🔒 Mutex:           432.284 ms                                  ║
║  📖 RwLock:          671.479 ms  (slower than Mutex)              ║
║  🔄 Spinlock:        372.371 ms  (1.2× faster)                   ║
║  ⚡ Lock-Free Atomic: 160.631 ms  (2.7× faster)                  ║
║  🧩 Bit-Packed CAS:  451.521 ms  (~1.0×, but 2 metrics)          ║
║                                                                   ║
║  🚀 Speedups vs Mutex:                                            ║
║     RwLock:           0.6×                                        ║
║     Spinlock:         1.2× faster                                 ║
║     Lock-Free Atomic: 2.7× faster                                 ║
║     Bit-Packed CAS:   ~1.0×                                       ║
║                                                                   ║
║  🎯 Observation:                                                   ║
║     RwLock is slower than Mutex for this write-heavy workload.    ║
║                                                                   ║
║  🚀 Trade-off:                                                     ║
║     Bit-Packed CAS sacrifices some throughput to maintain         ║
║     multiple metrics inside one atomic 64-bit value.              ║
╚═══════════════════════════════════════════════════════════════════╝
```

### Why This Matters

| Variant | Time | vs Mutex | Key Insight |
|---|---|---|---|
| 🔒 Mutex | 432 ms | 1.0× | Baseline locking approach |
| 📖 RwLock | 671 ms | 0.6× | Slower for this write-heavy workload |
| 🔄 Spinlock | 372 ms | 1.2× | Avoids blocking but still serializes updates |
| ⚡ Lock-Free Atomic | 160 ms | 2.7× | Hardware atomic operation provides the highest throughput |
| 🧩 Bit-Packed CAS | 451 ms | ~1.0× | Maintains two metrics atomically in one word |

> **Important:** Benchmark results are workload- and hardware-dependent. These numbers demonstrate the behavior of this particular implementation and workload rather than universally proving that one synchronization primitive is always faster.

---

## 🏗️ Architecture Deep-Dive

### The Bit-Packing Strategy

A single 64-bit atomic value stores two independent 32-bit metrics.

```rust
use std::sync::atomic::{AtomicU64, Ordering};

// One 64-bit integer holds TWO metrics.
let packed = AtomicU64::new(0);

// Packing:
// bits 0..31  -> total
// bits 32..63 -> spam
let spam = (packed.load(Ordering::Relaxed) >> 32) as u32;
let total = (packed.load(Ordering::Relaxed) & 0xFFFF_FFFF) as u32;
```

The update is performed using a compare-and-swap loop:

```rust
loop {
    let current = packed.load(Ordering::Acquire);

    let total = (current & 0xFFFF_FFFF) + 1;
    let spam = ((current >> 32) + 1) << 32;

    let target = spam | total;

    if packed
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

    // Another thread modified the value.
    // Retry with the latest value.
}
```

The important property is that both metrics transition together as one atomic 64-bit state update.

Under contention, the CAS operation may fail and retry. The benchmark measures the cost of that synchronization strategy in addition to the benefit of packing multiple values into a single atomic word.

### 🧠 Why Bit-Packing Is Useful

Bit-packing is particularly useful when several pieces of state:

- Are small enough to fit into a single machine word
- Must be observed or updated consistently
- Need lock-free access
- Are accessed at extremely high frequency

Instead of maintaining separate synchronization primitives for each metric, the entire state can be represented as:

```text
┌──────────────────────── AtomicU64 ────────────────────────┐
│                  32 bits                 │     32 bits     │
│              Spam / Flagged              │      Total      │
└──────────────────────────────────────────┴─────────────────┘
                         64 bits
```

This creates a compact atomic state machine that can be updated using a single CAS operation.

---

## ⚖️ Synchronization Strategy Comparison

| Property | Mutex | RwLock | Spinlock | Lock-Free Atomic | Bit-Packed CAS |
|---|---|---|---|---|---|
| OS blocking | Yes | Yes | No | No | No |
| Busy waiting | No | No | Yes | No | No |
| Atomic integrity | ✅ | ✅ | ✅ | ✅ | ✅ |
| Multi-metric update | ✅ | ✅ | ✅ | ❌ | ✅ |
| Single atomic word | ❌ | ❌ | ❌ | ✅ | ✅ |
| CAS required | ❌ | ❌ | ❌ | ❌ | ✅ |
| Complexity | Low | Medium | Medium | Low | High |
| Benchmark throughput | 🐌 | 🐢 | 🐢 | 🚀 | 🐢 |

---

## 🛠️ Quick Start

### Prerequisites

- Rust 1.97.1+
- Cargo

Install Rust with:

```bash
https://rustup.rs/
```

### Clone & Run

```bash
git clone https://github.com/yourusername/pack-vs-lock.git
cd pack-vs-lock
cargo run --release
```

> **Note:** The `--release` flag is critical when running performance benchmarks. Debug builds do not provide representative optimized performance.

### 📊 Run with Criterion

For detailed statistical analysis:

```bash
# Run all benchmarks
cargo bench

# Run a specific benchmark
cargo bench -- Mutex/8

# Open the generated HTML report
open target/criterion/report/index.html
```

Criterion provides statistical analysis, confidence intervals, regression detection, and historical comparisons between benchmark runs.

### ⚙️ Customize the Test

Adjust these constants in `main.rs`:

```rust
const TOTAL_INCREMENTS: u64 = 10_000_000;
const THREAD_COUNT: usize = 8;
```

Experiment with different:

- Operation counts
- Thread counts
- CPU architectures
- Synchronization strategies
- Atomic memory orderings

---

## 📁 Project Structure

```text
pack-vs-lock/
├── benches/
│   └── benchmark.rs     # Criterion benchmark suite
├── src/
│   └── main.rs          # Quick benchmark runner
├── Cargo.toml
├── README.md
└── LICENSE
```

---

## 🔬 Criterion Benchmarks

The project includes Criterion.rs benchmarks for statistical performance analysis.

Example output:

```text
Concurrency Strategies/Mutex/8
                        time:   [57.041 ms 57.869 ms 58.639 ms]
                        change: [-9.6289% -6.9993% -4.2994%] (p = 0.00 < 0.05)
                        Performance has improved.

Concurrency Strategies/Atomic/8
                        time:   [16.545 ms 16.792 ms 17.021 ms]
                        change: [-6.8834% -1.6523% +2.4809%] (p = 0.60 > 0.05)
                        No change in performance detected.

Concurrency Strategies/BitPacked/8
                        time:   [50.913 ms 52.419 ms 53.887 ms]
                        change: [-4.5644% +3.1737% +9.5841%] (p = 0.44 > 0.05)
                        No change in performance detected.
```

### Key Features

- 📊 Statistical analysis
- 📈 Scaling curves from 1 → 32 threads
- 🎯 Regression detection
- 🔄 Baseline comparisons
- 📊 Interactive HTML reports

---

## 📈 Thread Scaling Results

Criterion results across different thread counts:

| Threads | Mutex | RwLock | Spinlock | Atomic | Bit-Packed |
|---|---|---|---|---|---|
| 1 | 1.98 ms | 1.85 ms | 2.41 ms | 0.94 ms | 1.47 ms |
| 2 | 5.92 ms | 7.30 ms | 5.18 ms | 2.97 ms | 6.12 ms |
| 4 | 26.8 ms | 35.7 ms | 15.1 ms | 8.82 ms | 29.5 ms |
| 8 | 57.9 ms | 62.5 ms | 33.6 ms | 16.8 ms | 52.4 ms |
| 16 | 105 ms | 115 ms | 61.5 ms | 32.0 ms | 94.2 ms |
| 32 | 220 ms | 279 ms | 103 ms | 72.9 ms | 204 ms |

### Scaling Observations

The scaling curve shows:

- **Mutex / RwLock:** Performance degrades significantly as contention increases.
- **Spinlock:** Performs better than traditional blocking locks in this workload but still serializes access.
- **Lock-Free Atomic:** Provides the strongest scaling in the measured workload.
- **Bit-Packed CAS:** Retains lock-free semantics while paying additional CAS/packing overhead.

---

## 🎓 What You'll Learn

By running and modifying this benchmark, you'll gain practical experience with:

- **Synchronization overhead** — understanding the cost of coordinating concurrent updates.
- **Mutex vs RwLock** — why a read-write lock is not automatically faster.
- **Hardware atomics** — how instructions such as `LOCK XADD` and `LOCK CMPXCHG` enable atomic updates.
- **Bit-packing** — storing multiple pieces of state inside a single 64-bit word.
- **CAS loops** — understanding the retry pattern used by lock-free algorithms.
- **Memory ordering** — seeing how `Relaxed`, `Acquire`, and `AcqRel` affect synchronization semantics.
- **Contention** — understanding how multiple CPU cores compete for ownership of shared cache lines.

---

## 🚀 Applying This to RadixIP

This benchmark provides the foundation for experimenting with a lock-free token bucket in RadixIP.

The idea is to represent the complete bucket state using one `AtomicU64`:

```text
┌────────────────────── AtomicU64 ──────────────────────┐
│                 32 bits                │    32 bits   │
│            Last refill time            │    Tokens    │
└────────────────────────────────────────┴──────────────┘
```

Example implementation:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

const TOKEN_LIMIT: u32 = 100;

struct TokenBucket {
    bucket: AtomicU64,
}

impl TokenBucket {
    fn try_consume(&self, now: u32) -> bool {
        loop {
            let packed = self.bucket.load(Ordering::Acquire);

            let tokens = (packed & 0xFFFF_FFFF) as u32;
            let last_refill = (packed >> 32) as u32;

            // Refill logic.
            let new_tokens = if now.saturating_sub(last_refill) > 1 {
                TOKEN_LIMIT
            } else if tokens > 0 {
                tokens - 1
            } else {
                return false;
            };

            let new_packed =
                ((now as u64) << 32) | (new_tokens as u64);

            if self
                .bucket
                .compare_exchange_weak(
                    packed,
                    new_packed,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return true;
            }

            // Contention detected.
            // Retry using the latest state.
        }
    }
}
```

The key idea: no mutex is required to update the bucket's state. The timestamp and token count move together as one atomic state transition.

This pattern is especially interesting for high-throughput systems where a rate-limit decision may occur on every request.

---

## 🧪 The Surprising Truth About RwLock

### Conventional Wisdom

`RwLock` can outperform `Mutex` when many threads are reading shared state, because multiple readers can access the lock simultaneously.

### This Workload

This benchmark performs write-heavy operations. Every increment requires exclusive access. As a result, the read-sharing advantage of `RwLock` disappears while its additional synchronization bookkeeping remains.

The measured result:

```text
Mutex   → 432 ms
RwLock  → 671 ms
```

### Lesson

Choose synchronization primitives based on the workload, not their reputation. An `RwLock` can be an excellent choice for read-heavy workloads, but it is not automatically faster than a `Mutex`.

---

## 🔬 What This Benchmark Does — and Doesn't — Prove

This project is intentionally focused on a specific workload: highly contended concurrent updates.

The results do **not** mean:

- Lock-free code is always faster.
- Mutex is always slow.
- RwLock is always slower.
- Spinlocks are always a good optimization.
- CAS is always preferable.

Performance depends on factors including:

- CPU architecture
- Number of cores
- Thread scheduling
- Cache topology
- Memory ordering
- Contention level
- Critical-section size
- Workload read/write ratio
- NUMA topology

The purpose of this project is to measure these trade-offs rather than assume them.

---

## 📈 Contributing

Contributions are welcome! Potential areas to explore:

- [ ] Add false-sharing analysis with `perf`
- [ ] Port the benchmark to Go, Java, and C++
- [ ] Add visualization with gnuplot
- [ ] Add memory-barrier analysis
- [ ] Test on ARM architectures
- [ ] Compare different atomic memory orderings
- [ ] Add CPU affinity / thread pinning
- [ ] Measure CAS retry counts
- [ ] Add cache-miss measurements
- [ ] Compare different CPU core counts
- [ ] Add NUMA-aware benchmarks

---

## 📝 License

MIT License — use freely. Attribution appreciated.

---

## ⭐ Show Your Support

If this project helped you understand lock-free concurrency, atomics, or bit-packing:

- ⭐ Star the repository
- 🐦 Share your benchmark results
- 🔗 Link it in your RadixIP documentation
- 🧪 Run the benchmark on your own hardware and compare results

---

## 🙏 Acknowledgments

- The Rust community for making systems programming accessible
- The hardware engineers who built modern atomic CPU instructions
- Everyone who benchmarks before optimizing

---

*Built with ❤️ and 🔥 by engineers who know that locks are the enemy of scale.*

> "The best way to learn concurrency is to measure it."
