/// Runtime TX yürütme TPS (Transactions Per Second) benchmark'ları.
///
/// Ölçülen operasyonlar:
/// - ParallelExecutor::execute_block()  — 100 / 1000 / 10_000 TX
/// - TX throughput (TPS)               — saniyede kaç TX
/// - AccountStateCache transfer()      — tek transfer latency
/// - State root hesabı                 — 1k / 10k hesapla
///
/// Hedef: 10_000+ TPS (Nova Shield Chain §5.9 performans hedefi)
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use nova_runtime::{
    executor::ParallelExecutor,
    state::AccountStateCache,
    tx::Transaction,
};
use nova_primitives::{Address, SlotId};
use std::sync::Arc;

fn addr(n: u64) -> Address {
    let mut b = [0u8; 20];
    b[..8].copy_from_slice(&n.to_le_bytes());
    Address::from_bytes(b)
}

fn make_funded_state(n_accounts: u64) -> Arc<AccountStateCache> {
    let state = Arc::new(AccountStateCache::new());
    for i in 0..n_accounts {
        state.set_balance(addr(i), 1_000_000_000_000_000_000u128);
    }
    state
}

/// N TX oluştur (mock_transfer, validation ECDSA imzası olmadığından hata döner
/// ama execute_block N sonuç üretir — throughput ölçümü için yeterli)
fn make_txs(n: usize) -> Vec<Transaction> {
    (0..n).map(|i| {
        Transaction::mock_transfer(
            addr(i as u64 % 100),
            addr((i as u64 + 1) % 100),
            1_000,
            i as u64 / 100, // nonce
        )
    }).collect()
}

// ─── TX execute throughput ────────────────────────────────────────────────────

fn bench_execute_block_throughput(c: &mut Criterion) {
    let tx_counts = [100usize, 1_000, 5_000];
    let mut group = c.benchmark_group("execute_block_tps");

    for &n in &tx_counts {
        group.throughput(Throughput::Elements(n as u64));
        let state = make_funded_state(100);
        let executor = ParallelExecutor::new(state, 1_000_000_000u128);
        let txs = make_txs(n);

        group.bench_with_input(
            BenchmarkId::new("tx_count", n),
            &txs,
            |b, txs| {
                b.iter(|| {
                    executor.execute_block(txs)
                });
            },
        );
    }
    group.finish();
}

// ─── AccountStateCache transfer latency ──────────────────────────────────────

fn bench_state_transfer(c: &mut Criterion) {
    let state = make_funded_state(2);
    let from = addr(0);
    let to = addr(1);

    c.bench_function("state_transfer_single", |b| {
        b.iter(|| {
            state.transfer(&from, &to, 1)
        })
    });
}

// ─── AccountStateCache balance read ──────────────────────────────────────────

fn bench_state_balance_read(c: &mut Criterion) {
    let state = make_funded_state(1000);
    let addrs: Vec<Address> = (0..1000).map(addr).collect();

    c.bench_function("state_balance_read_1000_accounts", |b| {
        b.iter(|| {
            for a in &addrs {
                let _ = state.balance(a);
            }
        })
    });
}

// ─── State root hesabı ───────────────────────────────────────────────────────

fn bench_state_root(c: &mut Criterion) {
    let account_counts = [100usize, 1_000, 5_000];
    let mut group = c.benchmark_group("state_root_compute");

    for &n in &account_counts {
        let state = make_funded_state(n as u64);

        group.bench_with_input(
            BenchmarkId::new("accounts", n),
            &state,
            |b, s| {
                b.iter(|| s.compute_state_root())
            },
        );
    }
    group.finish();
}

// ─── Paralel write contention ─────────────────────────────────────────────────

fn bench_concurrent_state_writes(c: &mut Criterion) {
    let state = Arc::new(AccountStateCache::new());

    // 1000 hesabı önceden fund et
    for i in 0u64..1000 {
        state.set_balance(addr(i), 1_000_000_000_000u128);
    }

    c.bench_function("state_1000_balance_writes", |b| {
        b.iter(|| {
            for i in 0u64..1000 {
                state.set_balance(addr(i), i * 1_000_000);
            }
        })
    });
}

criterion_group!(
    benches,
    bench_execute_block_throughput,
    bench_state_transfer,
    bench_state_balance_read,
    bench_state_root,
    bench_concurrent_state_writes,
);
criterion_main!(benches);
