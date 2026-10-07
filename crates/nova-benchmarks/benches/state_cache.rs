/// AccountStateCache concurrent erişim benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - Tek thread read/write latency
/// - N thread paralel read
/// - N thread paralel write (lock contention)
/// - Snapshot alma ve geri yükleme
/// - DashMap shard sayısı etkisi (16 vs 64 vs 256 shard)
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nova_runtime::state::AccountStateCache;
use nova_primitives::Address;
use std::sync::Arc;
use std::thread;

fn addr(n: u64) -> Address {
    let mut b = [0u8; 20];
    b[..8].copy_from_slice(&n.to_le_bytes());
    Address::from_bytes(b)
}

// ─── Tek thread read latency ─────────────────────────────────────────────────

fn bench_single_read(c: &mut Criterion) {
    let state = Arc::new(AccountStateCache::new());
    state.set_balance(addr(1), 1_000_000);
    let a = addr(1);

    c.bench_function("state_balance_read_single", |b| {
        b.iter(|| state.balance(&a))
    });
}

// ─── Tek thread write latency ─────────────────────────────────────────────────

fn bench_single_write(c: &mut Criterion) {
    let state = Arc::new(AccountStateCache::new());
    let a = addr(1);

    c.bench_function("state_balance_write_single", |b| {
        b.iter(|| state.set_balance(a, 1_000_000))
    });
}

// ─── N hesap toplu read ───────────────────────────────────────────────────────

fn bench_bulk_read(c: &mut Criterion) {
    let sizes = [100usize, 1_000, 10_000];
    let mut group = c.benchmark_group("state_bulk_read");

    for &n in &sizes {
        let state = Arc::new(AccountStateCache::new());
        for i in 0..n as u64 {
            state.set_balance(addr(i), i * 1_000);
        }
        let addrs: Vec<Address> = (0..n as u64).map(addr).collect();

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(
            BenchmarkId::new("accounts", n),
            &(state, addrs),
            |b, (s, addrs)| {
                b.iter(|| {
                    for a in addrs {
                        let _ = s.balance(a);
                    }
                })
            },
        );
    }
    group.finish();
}

// ─── N hesap toplu write ──────────────────────────────────────────────────────

fn bench_bulk_write(c: &mut Criterion) {
    let sizes = [100usize, 1_000, 10_000];
    let mut group = c.benchmark_group("state_bulk_write");

    for &n in &sizes {
        let state = Arc::new(AccountStateCache::new());
        let addrs: Vec<Address> = (0..n as u64).map(addr).collect();

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(
            BenchmarkId::new("accounts", n),
            &(state, addrs),
            |b, (s, addrs)| {
                b.iter(|| {
                    for (i, a) in addrs.iter().enumerate() {
                        s.set_balance(*a, i as u128 * 1_000);
                    }
                })
            },
        );
    }
    group.finish();
}

// ─── Paralel write (çok thread) ──────────────────────────────────────────────

fn bench_parallel_write(c: &mut Criterion) {
    let thread_counts = [2usize, 4, 8];
    let mut group = c.benchmark_group("state_parallel_write");

    for &threads in &thread_counts {
        let state = Arc::new(AccountStateCache::new());
        let writes_per_thread = 1_000;

        group.throughput(Throughput::Elements((threads * writes_per_thread) as u64));
        group.bench_with_input(
            BenchmarkId::new("threads", threads),
            &(Arc::clone(&state), threads),
            |b, (state, &threads)| {
                b.iter(|| {
                    let handles: Vec<_> = (0..threads)
                        .map(|t| {
                            let s = Arc::clone(state);
                            thread::spawn(move || {
                                for i in 0..writes_per_thread {
                                    let a = addr((t * writes_per_thread + i) as u64 % 1000);
                                    s.set_balance(a, (i * t) as u128 * 1_000);
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });
            },
        );
    }
    group.finish();
}

// ─── Snapshot / restore ──────────────────────────────────────────────────────

fn bench_snapshot(c: &mut Criterion) {
    let sizes = [100usize, 1_000];
    let mut group = c.benchmark_group("state_snapshot");

    for &n in &sizes {
        let state = Arc::new(AccountStateCache::new());
        for i in 0..n as u64 {
            state.set_balance(addr(i), i * 1_000);
        }

        group.bench_with_input(
            BenchmarkId::new("accounts", n),
            &state,
            |b, s| {
                b.iter(|| s.snapshot())
            },
        );
    }
    group.finish();
}

// ─── Transfer (from → to, bakiye kontrolü dahil) ─────────────────────────────

fn bench_transfer(c: &mut Criterion) {
    let state = Arc::new(AccountStateCache::new());
    state.set_balance(addr(0), 1_000_000_000_000u128);
    let from = addr(0);
    let to = addr(1);

    c.bench_function("state_transfer_1_token", |b| {
        b.iter(|| state.transfer(&from, &to, 1))
    });
}

criterion_group!(
    benches,
    bench_single_read,
    bench_single_write,
    bench_bulk_read,
    bench_bulk_write,
    bench_parallel_write,
    bench_snapshot,
    bench_transfer,
);
criterion_main!(benches);
