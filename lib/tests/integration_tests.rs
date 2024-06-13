use ccsds::*;
use md5::{Digest, Md5};
use std::fs::{self, File};
use std::io::Error as IoError;
use std::path::PathBuf;
use std::result::Result;

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    path.push(name);
    path
}

#[test]
fn packet_iter() {
    let fpath = fixture_path("tests/fixtures/viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let iter = read_packets(reader);

    let packets: Vec<Result<Packet, IoError>> = iter.collect();

    assert_eq!(packets.len(), 100);
}

#[test]
fn group_iter() {
    let fpath = fixture_path("tests/fixtures/viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let iter = read_packet_groups(reader);
    let groups: Vec<Result<PacketGroup, IoError>> = iter.collect();

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

    let iter = read_synchronized_blocks(reader, &ASM[..].to_vec(), 1020);

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
    let blocks = Synchronizer::new(reader, &ASM.to_vec(), 1020)
        .into_iter()
        .filter_map(Result::ok);

    let frames: Vec<DecodedFrame> = FrameRSDecoder::builder()
        .interleave(4)
        .build()
        .decode(blocks)
        .filter_map(Result::ok)
        .collect();
    assert_eq!(frames.len(), 65, "expected frame count doesn't match");

    let packets: Vec<DecodedPacket> = decode_framed_packets(frames.into_iter(), 0, 0).collect();
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
    let groups: Vec<PacketGroup> = collect_packet_groups(packets.into_iter())
        .filter_map(Result::ok)
        .collect();

    assert_eq!(groups.len(), 2, "expected group count doesn't match");
}

#[test]
fn merge_test() {
    let tmpdir = tempfile::tempdir().unwrap();
    let out_path = tmpdir.path().join("output.dat");
    {
        let out_file = File::create(&out_path).unwrap();
        ccsds::merge_by_timecode(
            &[
                fixture_path("tests/fixtures/viirs_merge1.dat"),
                fixture_path("tests/fixtures/viirs_merge2.dat"),
            ],
            &ccsds::CDSTimeDecoder,
            out_file,
        )
        .unwrap();
    }

    // Get the merged files' packet groups, sorted
    let merged = File::open(&out_path).unwrap();
    let mut groups: Vec<ccsds::PacketGroup> = ccsds::read_packet_groups(merged)
        .filter_map(Result::ok)
        .collect();
    groups.sort_by(|a, b| a.apid.cmp(&b.apid));

    assert_eq!(groups.len(), 20, "expected 20 total groups");

    for i in 0..=6 {
        assert_eq!(groups[i].apid, 800, "group {i}");
        assert!(groups[i].valid(), "group {i}");
        assert_eq!(groups[i].packets.len(), 17, "group {i}");
    }
    for i in 7..=13 {
        assert_eq!(groups[i].apid, 801, "group {i}");
        assert!(groups[i].valid(), "group {i}");
        assert_eq!(groups[i].packets.len(), 17, "group {i}");
    }
    for i in 14..=19 {
        assert_eq!(groups[i].apid, 826, "group {i}");
        assert!(groups[i].valid(), "group {i}");
        assert_eq!(groups[i].packets.len(), 1, "group {i}");
    }
}
