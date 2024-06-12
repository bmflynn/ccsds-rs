use std::{fs::File, io::Read, path::PathBuf};

use ccsds::{DefaultPN, DefaultReedSolomon, PNDecoder, ReedSolomon};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(file!());
    path.pop();
    path.pop();
    path.push(name);
    path
}

use rand::Rng;

fn bench_rs_correct_codeblock(c: &mut Criterion) {
    // Read a single CADU from the fixture file
    let mut file = File::open(fixture_path("tests/fixtures/snpp_synchronized_cadus.dat")).unwrap();
    let mut block = [0u8; 1024]; // ASM + codeblock
    file.read_exact(&mut block).unwrap();
    let block = &block[4..];

    let mut group = c.benchmark_group("rs");
    group.throughput(Throughput::Bytes(1020));
    group.bench_function("correct_codeblock", |b| {
        b.iter(|| {
            let rs = DefaultReedSolomon;
            let _ = rs.correct_codeblock(block, 4).unwrap();
        });
    });
    group.finish();
}

// Pn decode a random slice of data.
fn bench_pndecode(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let mut buf = [0u8; 1020];
    for i in 0..buf.len() {
        let f: u8 = rng.gen();
        buf[i] = f;
    }

    let mut group = c.benchmark_group("pn");
    group.throughput(Throughput::Bytes(buf.len() as u64));
    group.bench_function("decode", |b| {
        b.iter(|| {
            let pn = DefaultPN;
            let _ = pn.decode(&buf.clone());
        });
    });
    group.finish();
}

criterion_group!(benches, bench_pndecode, bench_rs_correct_codeblock);
criterion_main!(benches);
