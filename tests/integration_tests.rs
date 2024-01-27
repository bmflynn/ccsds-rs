use ccsds::*;
use std::fs;
use std::io::Error as IoError;
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(file!());
    path.pop();
    path.pop();
    path.push(name.to_owned());
    path
}

#[test]
fn packet_iter() {
    let fpath = fixture_path("tests/fixtures/viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let iter = read_packets(Box::new(reader));

    let packets: Vec<Result<Packet, IoError>> = iter.collect();

    assert_eq!(packets.len(), 100);
}

#[test]
fn group_iter() {
    let fpath = fixture_path("tests/fixtures/viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let iter = read_packet_groups(Box::new(reader));

    let groups: Vec<Result<PacketGroup, IoError>> = iter.collect();

    assert_eq!(groups.len(), 7);

    // expected_counts is derived from edosl0util(0.16.0) collect_groups results
    let expected_counts = vec![1, 17, 17, 17, 17, 17, 14];
    for (idx, (group, expected_count)) in groups.iter().zip(expected_counts).enumerate() {
        let group = group.as_ref().unwrap();
        let count = group.packets.len();
        assert_eq!(
            count, expected_count,
            "Expected {} packets in group at index {}, got {}",
            expected_count, idx, count
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
    assert_eq!(count, 7, "expected 7 total cadus")
}

#[test]
#[ignore]
fn full_decode() {
    let fpath = fixture_path("tests/fixtures/snpp_synchronized_cadus.dat");
    let reader = fs::File::open(fpath).unwrap();

    let frames: Vec<DecodedFrame> = FrameDecoderBuilder::new(1024)
        .reed_solomon_interleave(4)
        .build(reader)
        .collect();
    assert_eq!(frames.len(), 65, "expected frame count doesn't match");

    let packets: Vec<Packet> =
        decode_framed_packets(157, Box::new(frames.into_iter()), 0, 0).collect();
    assert_eq!(packets.len(), 12, "expected packet count doesn't match");

    // The VIIRS sensor on Suomi-NPP uses packet grouping, so here we collect the packets
    // into their associated groups.
    let groups: Vec<PacketGroup> = collect_packet_groups(Box::new(packets.into_iter()))
        .filter_map(|zult| zult.ok())
        .collect();

    assert_eq!(groups.len(), 2, "expected group count doesn't match");
}
