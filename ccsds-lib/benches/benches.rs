use rand::{Rng, RngCore};
use std::path::PathBuf;

use ccsds::framing::{
    Block, DefaultDerandomizer, DefaultReedSolomon, Derandomizer, Integrity, ReedSolomon, SyncOpts,
    VCDUHeader,
};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    path.push(name);
    path
}

// fn bench_synchronization(c: &mut Criterion) {
//     let mut group = c.benchmark_group("sync");
//     group.throughput(Throughput::Bytes(1024));
//     group.bench_function("random_data", move |b| {
//         let mut data: [u8; 1024] = [0u8; 1024];
//         let mut rng = rand::thread_rng();
//         rng.fill_bytes(&mut data);
//
//         b.iter(move || {
//             let sync = ccsds::framing::synchronize(data, SyncOpts::new(1020));
//             let _: Vec<Block> = sync.into_iter().collect();
//         });
//     });
//
//     group.finish();
// }

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

    let header = VCDUHeader::decode(block).unwrap();
    let mut group = c.benchmark_group("rs");
    group.throughput(Throughput::Bytes(1020));
    group.bench_function("correct_codeblock", |b| {
        b.iter(|| {
            let rs = DefaultReedSolomon::new(4);
            let (i, _) = rs.perform(&header, block).unwrap();
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
fn bench_derandomize(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let mut buf = [0u8; 1020];
    for b in buf.iter_mut() {
        let f: u8 = rng.gen();
        *b = f;
    }

    let mut group = c.benchmark_group("derandomize");
    group.throughput(Throughput::Bytes(buf.len() as u64));
    group.bench_function("loop", |b| {
        b.iter(|| {
            let pn = DefaultDerandomizer;
            let _ = pn.derandomize(&buf.clone());
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_derandomize,
    bench_rs_correct_codeblock,
    // bench_synchronization
);
criterion_main!(benches);
