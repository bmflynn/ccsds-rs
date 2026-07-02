use std::{fs::File, io::Read};

use ccsds::spacepacket::{collect_groups, decode_packets};
pub use ccsds::spacepacket::{Packet, PacketGroup, PrimaryHeader};
use pyo3::prelude::*;

// FIXME: Remove "unsendable"
//        This may require some usage of Arc or something
#[pyclass(unsendable)]
pub struct PacketIter {
    packets: Box<dyn Iterator<Item = Packet> + Send>,
}

#[pymethods]
impl PacketIter {
    pub fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    pub fn __next__(mut slf: PyRefMut<Self>) -> Option<Packet> {
        slf.packets.next()
    }
}

/// Decode packets from a local file.
///
/// Args:
///     path: Path to a local file on disk
///
/// Returns:
///     Iterator of decoded Packets.
#[pyfunction]
#[pyo3(name = "decode_packets")]
pub fn py_decode_packets(path: &str) -> PyResult<PacketIter> {
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets: Box<dyn Iterator<Item = Packet> + Send + 'static> =
        Box::new(decode_packets(file).filter_map(|z| z.ok()));

    Ok(PacketIter { packets })
}

#[pyclass(unsendable)]
pub struct PacketGroupIter {
    groups: Box<dyn Iterator<Item = PacketGroup> + Send>,
}

#[pymethods]
impl PacketGroupIter {
    pub fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    pub fn __next__(mut slf: PyRefMut<Self>) -> Option<PacketGroup> {
        slf.groups.next()
    }
}

/// Decode PacketGroups according to their primiary header grouping flag.
///
/// Each group will contain all packets that can be identified as part of that group. Any
/// standalone packets will be a group of 1. Groups do not need be complete, i.e., start with a
/// first and end with a last.
///
/// Args:
///     path: Path to a local file on disk
///
/// Returns: An iterable of PacketGroups
#[pyfunction]
pub fn decode_packet_groups(path: &str) -> PyResult<PacketGroupIter> {
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets = decode_packets(file).filter_map(Result::ok);
    let groups = Box::new(collect_groups(packets).filter_map(Result::ok));
    Ok(PacketGroupIter { groups })
}
