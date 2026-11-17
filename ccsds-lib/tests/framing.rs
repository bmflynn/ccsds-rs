mod common;

use std::{collections::HashMap, fs::File};

use ccsds::framing::*;
use common::fixture_path;

fn do_framing_test(interleave: u8, block_len: usize, fixture: &str, expected: &[(Vcid, usize)]) {
    let file = File::open(fixture_path(fixture)).expect("Failed to open fixture file");
    let cadus = synchronize(file, SyncOpts::new(block_len)).map_while(Result::ok);
    let cadus = derandomize(cadus);
    let frames = frame_decoder(cadus);
    let frames = reed_solomon(frames, RsOpts::new(interleave));

    let mut got_counts: HashMap<Vcid, usize> = HashMap::default();
    for frame in frames {
        let cur = got_counts.entry(frame.header.vcid).or_default();
        *cur += 1;
    }

    for &(vcid, expected) in expected {
        let got = got_counts.get(&vcid).unwrap_or(&0);
        assert_eq!(
            *got, expected,
            "Expected {expected} for vcid {vcid}, got {got} input {fixture}",
        );
    }
}

#[test]
fn test_framing_snpp_4_1020() {
    do_framing_test(
        4,
        1020,
        "cadu/npp.20241206T173815.dat",
        &[(16, 945), (63, 78)],
    );
}

#[test]
fn test_framing_noaa20_4_1020() {
    do_framing_test(
        4,
        1020,
        "cadu/noaa20.20241206T162710.dat",
        &[(16, 943), (1, 1), (6, 79)],
    );
}

#[test]
fn test_framing_noaa21_5_1275() {
    do_framing_test(
        5,
        1275,
        "cadu/noaa21.20241206T171609.dat",
        &[(0, 10), (1, 1), (6, 89), (63, 719)],
    );
}

#[test]
fn test_framing_metopb_4_1020() {
    do_framing_test(
        5,
        1275,
        "cadu/metopb.20241206T152751.dat",
        &[
            (10, 264),
            (34, 2),
            (12, 3),
            (63, 63),
            (15, 4),
            (27, 1),
            (29, 6),
            (9, 103),
            (24, 66),
        ],
    );
}

#[test]
fn test_framing_metopc_4_1020() {
    do_framing_test(
        4,
        1020,
        "cadu/metopc.20241206T162917.dat",
        &[(6, 93), (1, 1), (16, 929)],
    );
}

#[test]
fn test_framing_aqua_4_1020() {
    do_framing_test(
        4,
        1020,
        "cadu/aqua.20241206T175646.dat",
        &[(63, 63), (5, 9), (30, 828), (35, 121), (10, 2)],
    );
}

#[test]
fn test_demux_packets_have_vcid() {
    let file = File::open(fixture_path("cadu/npp.20241206T173815.dat")).unwrap();
    let cadus = synchronize(file, SyncOpts::new(1020)).map_while(Result::ok);
    let frames = reed_solomon(frame_decoder(derandomize(cadus)), RsOpts::new(4));

    let mut count = 0;
    for packet in PacketDemux::new(frames).with_defaults(0, false, false, true) {
        let vcid = packet.vcid.expect("demux packets must have a vcid");
        assert!(vcid == 16 || vcid == 63, "unexpected vcid {vcid}");
        count += 1;
    }
    assert!(count > 0);
}
