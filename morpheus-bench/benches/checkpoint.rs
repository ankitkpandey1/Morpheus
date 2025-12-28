//! Criterion benchmarks for checkpoint overhead

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use morpheus_runtime::checkpoint_sync;

fn checkpoint_benchmark(c: &mut Criterion) {
    c.bench_function("checkpoint_sync (no SCB)", |b| {
        b.iter(|| black_box(checkpoint_sync()))
    });
}

criterion_group!(benches, checkpoint_benchmark);
criterion_main!(benches);
