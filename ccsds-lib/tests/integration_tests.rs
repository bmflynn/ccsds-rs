use ccsds::framing::{
    decode_framed_packets, read_synchronized_blocks, DecodedFrame, DecodedPacket,
    DefaultDerandomizer, DefaultReedSolomon, FrameDecoder, Synchronizer, ASM,
};
use ccsds::timecode;
use md5::{Digest, Md5};
use std::fs::{self, File};
use std::path::PathBuf;
use std::result::Result;

use ccsds::spacepacket::{
    collect_groups, decode_packets, Error, Merger, Packet, PacketGroup, TimecodeDecoder,
};

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    path.push(name);
    path
}

#[test]
fn packet_iter() {
    let fpath = fixture_path("tests/fixtures/viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let iter = decode_packets(reader);

    let packets: Vec<Result<Packet, Error>> = iter.collect();

    assert_eq!(packets.len(), 100);
}

#[test]
fn group_iter() {
    let fpath = fixture_path("tests/fixtures/viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let packets = decode_packets(reader).map(Result::unwrap);
    let iter = collect_groups(packets);
    let groups: Vec<Result<PacketGroup, Error>> = iter.collect();

    assert_eq!(groups.len(), 7);

    // expected_counts is derived from edosl0util(0.16.0) collect_groups results
    let expected_counts = vec![1, 17, 17, 17, 17, 17, 14];
    for (idx, (group, expected_count)) in groups.iter().zip(expected_counts).enumerate() {
        let group = group.as_ref().unwrap();
        let count = group.packets.len();
        assert_eq!(
            count, expected_count,
            "Expected {expected_count} packets in group at index {idx}, got {count}",
        );
    }
}

#[test]
fn block_iter() {
    let fpath = fixture_path("tests/fixtures/snpp_7cadus_2vcids.dat");
    let reader = fs::File::open(fpath).unwrap();

    let iter = read_synchronized_blocks(reader, &ASM[..], 1020);

    let mut count = 0;
    for zult in iter {
        zult.unwrap();
        count += 1;
    }
    assert_eq!(count, 7, "expected 7 total cadus");
}

#[test]
fn full_decode() {
    let fpath = fixture_path("tests/fixtures/snpp_synchronized_cadus.dat");
    let reader = fs::File::open(fpath).unwrap();
    let blocks = Synchronizer::new(reader, &ASM, 1020)
        .into_iter()
        .map(Result::unwrap);

    let rs = DefaultReedSolomon::new(4);
    let frames: Vec<DecodedFrame> = FrameDecoder::new()
        .with_integrity(Box::new(rs))
        .with_derandomization(Box::new(DefaultDerandomizer))
        .decode(blocks)
        .map(Result::unwrap)
        .collect();
    assert_eq!(frames.len(), 65, "expected frame count doesn't match");

    let packets: Vec<DecodedPacket> = decode_framed_packets(frames.into_iter(), 0, 0).collect();
    for p in &packets {
        println!("{:?}", p.packet.header);
    }
    assert_eq!(packets.len(), 12, "expected packet count doesn't match");

    let mut hasher = Md5::new();
    packets.iter().for_each(|p| hasher.update(&p.packet.data));
    let result = hasher.finalize();
    assert_eq!(
        result[..],
        hex::decode("5e11051d86c46ddc3500904c99bbe978").expect("bad fixture checksum"),
        "output checksum does not match fixture file checksum"
    );

    // The VIIRS sensor on Suomi-NPP uses packet grouping, so here we collect the packets
    // into their associated groups.
    let packets: Vec<Packet> = packets.iter().map(|p| p.packet.clone()).collect();
    let groups: Vec<PacketGroup> = collect_groups(packets.into_iter())
        .map(Result::unwrap)
        .collect();

    assert_eq!(groups.len(), 2, "expected group count doesn't match");
}

#[test]
fn merge_test() {
    let tmpdir = tempfile::tempdir().unwrap();
    let out_path = tmpdir.path().join("output.dat");
    let out_file = File::create(&out_path).unwrap();
    Merger::new(
        vec![
            fixture_path("tests/fixtures/viirs_merge1.dat"),
            fixture_path("tests/fixtures/viirs_merge2.dat"),
        ],
        TimecodeDecoder::new(timecode::Format::Cds {
            num_day: 2,
            num_submillis: 2,
        }),
    )
    .merge(out_file)
    .unwrap();

    // Get the merged files' packet groups, sorted
    let merged = File::open(&out_path).unwrap();
    let packets: Vec<Packet> = decode_packets(merged).map(Result::unwrap).collect();
    assert_eq!(
        packets.len(),
        235,
        "Expected 235 packets, got {}",
        packets.len()
    );

    let mut groups: Vec<PacketGroup> = collect_groups(packets.into_iter())
        .map(Result::unwrap)
        .collect();
    assert_eq!(groups.len(), 20, "expected 20 total groups");

    groups.sort_by(|a, b| a.apid.cmp(&b.apid));
    for (i, group) in groups.iter().take(7).enumerate() {
        assert_eq!(group.apid, 800, "group {i} has wrong apid");
        assert!(group.complete(), "group {i} should be complete");
        assert_eq!(group.packets.len(), 17, "group {i} has wrong len");
    }
    // NOTE: last 801 group is incomplete, so we skip it
    for (i, group) in groups.iter().skip(7).take(5).enumerate() {
        assert_eq!(group.apid, 801, "group {i} has wrong apid");
        for p in &group.packets {
            println!("{:?}", p.header);
        }
        assert!(group.complete(), "group {i} should be complete");
        assert_eq!(group.packets.len(), 17, "group {i} has wrong len");
    }
    for (i, group) in groups.iter().skip(14).enumerate() {
        assert_eq!(group.apid, 826, "group {i} has wrong apid");
        assert!(group.complete(), "group {i} should be complete");
        assert_eq!(group.packets.len(), 1, "group {i} has wrong len");
    }
}
