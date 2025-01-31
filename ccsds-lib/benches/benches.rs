use rand::{Rng, RngCore};
use std::path::PathBuf;

use ccsds::framing::{
    DefaultDerandomizer, DefaultReedSolomon, Derandomizer, Integrity, IntegrityAlgorithm,
    Synchronizer, ASM,
};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    path.push(name);
    path
}

fn bench_synchronization(c: &mut Criterion) {
    let data: [u8; 1024] = {
        let mut x = [0u8; 1024];
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut x);
        x
    };

    let mut group = c.benchmark_group("sync");
    group.throughput(Throughput::Bytes(1024));
    group.bench_function("random_data", |b| {
        b.iter(|| {
            let sync = Synchronizer::new(&data[..], &ASM, 1024);
            let _: Vec<Vec<u8>> = sync.into_iter().filter_map(Result::ok).collect();
        });
    });

    group.finish();
}

fn bench_rs_correct_codeblock(c: &mut Criterion) {
    let mut block = std::fs::read(fixture_path("benches/snpp_block.dat")).unwrap();

    // introduced some errors
    let block = {
        let mut rng = rand::thread_rng();
        let b = &mut block[4..];
        let idx: usize = rng.gen::<u8>().into();
        b[idx] = b[idx].wrapping_add(1);
        b
    };

    let mut group = c.benchmark_group("rs");
    group.throughput(Throughput::Bytes(1020));
    group.bench_function("correct_codeblock", |b| {
        b.iter(|| {
            let rs = DefaultReedSolomon::new(4);
            let (i, _) = rs.perform(block).unwrap();
            assert_eq!(
                i,
                Integrity::Corrected,
                "expected to have corrected block; got {i:?}"
            );
        });
    });
    group.finish();
}

// Pn decode a random slice of data.
fn bench_pndecode(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let mut buf = [0u8; 1020];
    for b in buf.iter_mut() {
        let f: u8 = rng.gen();
        *b = f;
    }

    let mut group = c.benchmark_group("pn");
    group.throughput(Throughput::Bytes(buf.len() as u64));
    group.bench_function("decode", |b| {
        b.iter(|| {
            let pn = DefaultDerandomizer;
            let _ = pn.derandomize(&buf.clone());
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_pndecode,
    bench_rs_correct_codeblock,
    bench_synchronization
);
criterion_main!(benches);
