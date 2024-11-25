use std::{fs::File, io::Read};

use ccsds::{
    prelude::Apid,
    spacepacket::{collect_groups, decode_packets, Packet, PacketGroup, PrimaryHeader},
    timecode::Format,
};
use pyo3::{prelude::*, types::PyBytes};

#[pyclass(name = "PrimaryHeader")]
pub struct PyPrimaryHeader {
    header: PrimaryHeader,
}

impl PyPrimaryHeader {
    fn from_header(header: PrimaryHeader) -> Self {
        PyPrimaryHeader { header }
    }
}

#[pymethods]
impl PyPrimaryHeader {
    pub fn __str__(&self) -> String {
        format!("{:?}", self.header)
    }
    #[new]
    pub fn new(py: Python, buf: Py<PyBytes>) -> PyResult<Self> {
        let buf = buf.as_bytes(py);
        Ok(PyPrimaryHeader::from_header(PrimaryHeader::decode(buf)?))
    }
    pub fn version(&self) -> u8 {
        self.header.version
    }
    pub fn type_flag(&self) -> u8 {
        self.header.type_flag
    }
    pub fn has_secondary_header(&self) -> bool {
        self.header.has_secondary_header
    }
    pub fn apid(&self) -> Apid {
        self.header.apid
    }
    pub fn sequence_flags(&self) -> u8 {
        self.header.sequence_flags
    }
    pub fn sequence_id(&self) -> u16 {
        self.header.sequence_id
    }
    pub fn len_minus1(&self) -> u16 {
        self.header.len_minus1
    }
}

#[pyclass(name = "Packet")]
pub struct PyPacket {
    packet: Packet,
}

impl PyPacket {
    fn from_packet(packet: Packet) -> Self {
        Self { packet }
    }
}

#[pymethods]
impl PyPacket {
    pub fn __str__(&self) -> String {
        format!("{}", self.packet)
    }

    #[new]
    pub fn new(py: Python, buf: Py<PyBytes>) -> PyResult<Self> {
        let buf = buf.as_bytes(py);
        Ok(PyPacket::from_packet(Packet::decode(buf)?))
    }

    pub fn header(&self) -> PyPrimaryHeader {
        PyPrimaryHeader::from_header(self.packet.header)
    }

    /// All packet data
    pub fn data(&self) -> Vec<u8> {
        self.packet.data.clone()
    }

    /// User data, i.e., no primary header data
    pub fn user_data(&self) -> Vec<u8> {
        self.packet.data[6..].to_vec()
    }
}

#[pyclass(name = "PacketGroup")]
struct PyPacketGroup {
    group: PacketGroup,
}

impl PyPacketGroup {
    fn from_group(group: PacketGroup) -> Self {
        Self { group }
    }
}

#[pymethods]
impl PyPacketGroup {
    pub fn __str__(&self) -> String {
        format!(
            "PacketGroup {{apid:{} packets[len={}]}}",
            self.group.apid,
            self.group.packets.len()
        )
    }
    pub fn apid(&self) -> Apid {
        self.group.apid
    }
    pub fn packets(&self) -> Vec<PyPacket> {
        self.group
            .packets
            .iter()
            // FIXME: Can we avoid clone?
            .map(|p| PyPacket::from_packet(p.clone()))
            .collect::<Vec<PyPacket>>()
    }
    pub fn complete(&self) -> bool {
        self.group.complete()
    }
    pub fn have_missing(&self) -> bool {
        self.group.have_missing()
    }
}

#[pyclass(unsendable)]
struct PyPacketIter {
    packets: Box<dyn Iterator<Item = Packet> + Send>,
}

#[pymethods]
impl PyPacketIter {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<Py<PyPacket>> {
        match slf.packets.next() {
            Some(packet) => Py::new(slf.py(), PyPacket::from_packet(packet)).ok(),
            None => None,
        }
    }
}

/// Decode packets from a local file.
///
/// Returns
/// -------
/// Iterator of `Packet`s
#[pyfunction]
#[pyo3(name = "decode_packets")]
fn py_decode_packets(path: &str) -> PyResult<PyPacketIter> {
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets: Box<dyn Iterator<Item = Packet> + Send + 'static> =
        Box::new(decode_packets(file).filter_map(|z| z.ok()));

    Ok(PyPacketIter { packets })
}

#[pyclass(unsendable)]
struct PyPacketGroupIter {
    groups: Box<dyn Iterator<Item = PacketGroup> + Send>,
}

#[pymethods]
impl PyPacketGroupIter {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<Py<PyPacketGroup>> {
        match slf.groups.next() {
            Some(group) => Py::new(slf.py(), PyPacketGroup::from_group(group)).ok(),
            None => None,
        }
    }
}

/// Decode PacketGroups according to their primiary header grouping flag.
///
/// Each group will contain all packets that can be identified as part of that group. Any
/// standalone packets will be a group of 1. Groups do not need be complete, i.e., start with a
/// first and end with a last.
///
/// Returns
/// -------
/// An iterable of PacketGroups
#[pyfunction]
fn decode_packet_groups(path: &str) -> PyResult<PyPacketGroupIter> {
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets = decode_packets(file).filter_map(Result::ok);
    let groups = Box::new(collect_groups(packets).filter_map(Result::ok));
    Ok(PyPacketGroupIter { groups })
}

/// Decode `buf` into a CCSDS Day-segmented timecode.
///
/// Parameters
/// ----------
/// num_day: int
///     Number of bytes composing the day segment. Must be 1 to 4.
/// num_submillis: int
///     Number of bytes composing the submillisecond segment. Must be 0 to 4;
/// buf: bytes
///     Data to decode
///
/// Returns
/// -------
/// int
///     Number of microseconds since CCSDS epoch (Jan 1, 1958)
#[pyfunction]
fn decode_cds_timecode(num_day: usize, num_submillis: usize, buf: Vec<u8>) -> PyResult<u64> {
    let fmt = Format::Cds {
        num_day,
        num_submillis,
    };
    Ok(ccsds::timecode::decode(&fmt, &buf)?.nanos()? / 1000)
}

/// Decode `buf` into a CCSDS Unsegmented timecode.
///
/// Parameters
/// ----------
/// num_coarse: int
///     Number of bytes of coarse time (days)
/// num_submillis: int
///     Number of bytes of fine time
/// buf: bytes
///     Data to decode
/// fine_mult: float | None
///     Multiplier to convert time to nanoseconds, if any
///
/// Returns
/// -------
/// int
///     Number of microseconds since CCSDS epoch (Jan 1, 1958)
#[pyfunction]
#[pyo3(signature=(num_coarse, num_fine, buf, fine_mult=None))]
fn decode_cuc_timecode(
    num_coarse: usize,
    num_fine: usize,
    buf: Vec<u8>,
    fine_mult: Option<f32>,
) -> PyResult<u64> {
    let fmt = Format::Cuc {
        num_coarse,
        num_fine,
        fine_mult,
    };
    Ok(ccsds::timecode::decode(&fmt, &buf)?.nanos()? / 1000)
}

/// Decode JPSS CDS timecode
///
/// See `decode_cds_timecode`
#[pyfunction]
fn decode_jpss_timecode(buf: Vec<u8>) -> PyResult<u64> {
    decode_cds_timecode(2, 2, buf)
}

/// Decode JPSS CDS to microseconds since Jan 1, 1970
///
/// See `decode_cds_timecode`
#[pyfunction]
fn decode_jpss_timestamp(buf: Vec<u8>) -> PyResult<u64> {
    let tc = decode_jpss_timecode(buf)?;
    Ok(to_timestamp(tc))
}

/// Decode NASA EOS Telemetry CUC timecode
///
/// See `decode_cuc_timecode`
#[pyfunction]
fn decode_eos_timecode(buf: Vec<u8>) -> PyResult<u64> {
    decode_cuc_timecode(2, 4, buf, Some(15200.0))
}

/// Decode NASA EOS Telemetry CUC to microseconds since Jan 1, 1970
///
/// See `decode_cuc_timecode`
#[pyfunction]
fn decode_eos_timestamp(buf: Vec<u8>) -> PyResult<u64> {
    let tc = decode_eos_timecode(buf)?;
    Ok(to_timestamp(tc))
}

fn to_timestamp(timecode: u64) -> u64 {
    timecode - 378_691_200_000_000
}

/// ccsds
///
/// Python wrapper for the [ccsds](https://github.com/bmflynn/ccsds) Rust crate
/// providing decode capabilities for frames (sync, RS, pn, etc ...) and spacepackets.
#[pymodule]
#[pyo3(name = "ccsds")]
fn ccsdspy(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_decode_packets, m)?)?;
    m.add_function(wrap_pyfunction!(decode_packet_groups, m)?)?;

    m.add_function(wrap_pyfunction!(decode_cds_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_jpss_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_jpss_timestamp, m)?)?;
    m.add_function(wrap_pyfunction!(decode_cuc_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_eos_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_eos_timestamp, m)?)?;

    m.add_class::<PyPacket>()?;
    m.add_class::<PyPrimaryHeader>()?;
    m.add_class::<PyPacketGroup>()?;

    Ok(())
}
