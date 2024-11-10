use ccsds as my;
use ccsds::timecode;
use ccsds::{PNDecoder, ReedSolomon};
use pyo3::types::PyList;
use pyo3::{
    exceptions::{PyStopIteration, PyValueError},
    prelude::*,
    types::{PyBytes, PyType},
};
use std::{fs::File, io::Read};

#[pyclass]
struct BlockIterator {
    blocks: Box<dyn Iterator<Item = Vec<u8>> + Send>,
}

#[pymethods]
impl BlockIterator {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__<'a>(
        mut slf: PyRefMut<Self>,
        py: Python<'a>,
    ) -> PyResult<Option<Bound<'a, PyBytes>>> {
        match slf.blocks.next() {
            Some(block) => Ok(Some(PyBytes::new_bound(py, &block))),
            None => Err(PyStopIteration::new_err(String::new())),
        }
    }
}

/// Returns an iterator over blocks in the stream of size `block_size` located using the ASM byte
/// sequence. The stream need not be contiguious or byte-aligned.
#[pyfunction(signature=(source, block_size, asm=None))]
fn synchronized_blocks(
    source: &str,
    block_size: usize,
    asm: Option<&[u8]>,
) -> PyResult<BlockIterator> {
    let file: Box<dyn Read + Send> = Box::new(File::open(source)?);
    let asm = asm.unwrap_or(&my::ASM);

    let blocks = my::Synchronizer::new(file, asm, block_size)
        .into_iter()
        .filter_map(Result::ok);
    let iter = BlockIterator {
        blocks: Box::new(blocks),
    };
    Ok(iter)
}

/// Remove pseudo-noise.
///
/// Parameters
/// ----------
/// dat : list, bytes
///     Data to decode.
///
/// Raises
/// ------
/// ValueError
///     If the provided data is longer than the internal PN LUT
#[pyfunction(signature=(dat))]
fn pndecode<'a>(py: Python<'a>, dat: &[u8]) -> PyResult<Bound<'a, PyBytes>> {
    if dat.len() >= 1276 {
        return Err(PyValueError::new_err(
            "PN data longer than 1275 bytes".to_string(),
        ));
    }
    let dat = my::DefaultPN {}.decode(dat);
    let bytes = PyBytes::new_bound(py, &dat);
    Ok(bytes)
}

/// Perform RS on a single codeblock returning a tuple of the input data with parity bytes
/// removed and the RS disposition.
#[pyfunction(signature=(block, interleave))]
fn rs_correct_codeblock<'a>(
    py: Python<'a>,
    block: &[u8],
    interleave: u8,
) -> PyResult<(Bound<'a, PyBytes>, RSState)> {
    let rs = my::DefaultReedSolomon {};

    match rs.correct_codeblock(block, interleave) {
        Ok((block, state)) => {
            let bytes = PyBytes::new_bound(py, &block);
            Ok((bytes, state.into()))
        }
        Err(err) => Err(PyValueError::new_err(format!("rs failure: {err}"))),
    }
}

#[pyclass]
#[derive(Clone, Debug)]
struct PrimaryHeader {
    #[pyo3(get)]
    version: u8,
    #[pyo3(get)]
    type_flag: u8,
    #[pyo3(get)]
    has_secondary_header: bool,
    #[pyo3(get)]
    apid: u16,
    #[pyo3(get)]
    sequence_flags: u8,
    #[pyo3(get)]
    sequence_id: u16,
    #[pyo3(get)]
    len_minus1: u16,
}

#[pymethods]
impl PrimaryHeader {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        format!(
            "PrimaryHeader(version={}, type_flag={}, has_secondary_header={}, apid={}, sequence_flags={}, sequence_id={}, len_minus1={})",
            self.version, self.type_flag, self.has_secondary_header, self.apid, self.sequence_flags, self.sequence_id, self.len_minus1,
        ).to_owned()
    }

    #[classmethod]
    fn decode(_cls: &Bound<'_, PyType>, dat: &[u8]) -> Option<Self> {
        my::PrimaryHeader::decode(dat).map(|hdr| Self {
            version: hdr.version,
            type_flag: hdr.type_flag,
            has_secondary_header: hdr.has_secondary_header,
            apid: hdr.apid,
            sequence_flags: hdr.sequence_flags,
            sequence_id: hdr.sequence_id,
            len_minus1: hdr.len_minus1,
        })
    }
}

#[pyclass]
struct Packet {
    #[pyo3(get)]
    header: PrimaryHeader,
    #[pyo3(get)]
    data: Py<PyBytes>,
}

#[pymethods]
impl Packet {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        Python::with_gil(|py| {
            format!(
                "Packet(header={}, data_len={})",
                self.header.__str__(),
                self.data.as_bytes(py).len(),
            )
            .to_owned()
        })
    }
    #[classmethod]
    fn decode(_cls: Bound<'_, PyType>, dat: &[u8]) -> Option<Self> {
        my::Packet::decode(dat).map(Packet::new)
    }
}

impl Packet {
    fn new(packet: my::Packet) -> Self {
        Python::with_gil(|py| Packet {
            header: PrimaryHeader {
                version: packet.header.version,
                type_flag: packet.header.type_flag,
                has_secondary_header: packet.header.has_secondary_header,
                apid: packet.header.apid,
                sequence_flags: packet.header.sequence_flags,
                sequence_id: packet.header.sequence_id,
                len_minus1: packet.header.len_minus1,
            },
            data: PyBytes::new_bound(py, &packet.data).unbind(),
        })
    }
}

#[pyclass]
struct DecodedPacket {
    #[pyo3(get)]
    scid: u16,
    vcid: u16,
    packet: Packet,
}

#[pymethods]
impl DecodedPacket {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        format!(
            "DecodedPacket(scid={}, vcid={}, packet={})",
            self.scid,
            self.vcid,
            self.packet.__str__(),
        )
        .to_owned()
    }
}

impl DecodedPacket {
    fn new(packet: my::DecodedPacket) -> Self {
        DecodedPacket {
            scid: packet.scid,
            vcid: packet.vcid,
            packet: Packet::new(packet.packet),
        }
    }
}

#[pyclass]
struct PacketIterator {
    packets: Box<dyn Iterator<Item = my::Packet> + Send>,
}

#[pymethods]
impl PacketIterator {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<Py<Packet>> {
        match slf.packets.next() {
            Some(packet) => Py::new(slf.py(), Packet::new(packet)).ok(),
            None => None,
        }
    }
}

/// Decode space packet data from the provided source.
///
/// Parameters
/// ----------
/// source : str
///     Source providing stream of space packets to decode. Currently only local
///     file paths are supported.
///
/// Returns
/// -------
///     Iterator of Packets
#[pyfunction]
fn decode_packets(source: PyObject) -> PyResult<PacketIterator> {
    let path = match Python::with_gil(|py| -> PyResult<String> { source.extract(py) }) {
        Ok(s) => s,
        Err(e) => return Err(e),
    };

    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets: Box<dyn Iterator<Item = my::Packet> + Send + 'static> =
        Box::new(my::read_packets(file).filter_map(Result::ok));

    Ok(PacketIterator { packets })
}

#[pyclass]
struct DecodedPacketIterator {
    packets: Box<dyn Iterator<Item = my::DecodedPacket> + Send>,
}

#[pymethods]
impl DecodedPacketIterator {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<Py<DecodedPacket>> {
        match slf.packets.next() {
            Some(packet) => Py::new(slf.py(), DecodedPacket::new(packet)).ok(),
            None => None,
        }
    }
}

#[pyfunction]
fn decode_packet_groups(source: PyObject) -> PyResult<PacketGroupIterator> {
    let path = match Python::with_gil(|py| -> PyResult<String> { source.extract(py) }) {
        Ok(s) => s,
        Err(e) => return Err(e),
    };
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let groups: Box<dyn Iterator<Item = my::PacketGroup> + Send + 'static> =
        Box::new(my::read_packet_groups(file).filter_map(Result::ok));

    Ok(PacketGroupIterator { groups })
}

#[pyclass]
struct PacketGroup {
    #[pyo3(get)]
    apid: u16,
    #[pyo3(get)]
    packets: Py<PyList>,
}

#[pyclass]
struct PacketGroupIterator {
    groups: Box<dyn Iterator<Item = my::PacketGroup> + Send>,
}

#[pymethods]
impl PacketGroupIterator {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<Py<PacketGroup>> {
        match slf.groups.next() {
            Some(group) => Python::with_gil(|py| {
                let packets: Vec<Py<Packet>> = group
                    .packets
                    .into_iter()
                    .filter_map(|p| Py::new(py, Packet::new(p)).ok())
                    .collect();
                Py::new(
                    py,
                    PacketGroup {
                        apid: group.apid,
                        packets: PyList::new_bound(py, packets).unbind(),
                    },
                )
                .ok()
            }),
            None => None,
        }
    }
}

#[pyclass(eq, eq_int)]
#[derive(Clone, Debug, PartialEq)]
enum RSState {
    Ok,
    Corrected,
    Uncorrectable,
    NotPerformed,
}

#[pymethods]
impl RSState {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        match self {
            Self::Ok => "ok",
            Self::Corrected => "corrected",
            Self::Uncorrectable => "uncorrectable",
            Self::NotPerformed => "notperformed",
        }
        .to_owned()
    }
}

impl From<my::RSState> for RSState {
    fn from(value: my::RSState) -> Self {
        use my::RSState::*;
        match value {
            Ok => Self::Ok,
            my::RSState::Corrected(_) => Self::Corrected,
            my::RSState::Uncorrectable(_) => Self::Uncorrectable,
            my::RSState::NotPerformed => Self::NotPerformed,
        }
    }
}

#[pyclass]
#[derive(Clone, Debug)]
struct VCDUHeader {
    #[pyo3(get)]
    version: u8,
    #[pyo3(get)]
    scid: u16,
    #[pyo3(get)]
    vcid: u16,
    #[pyo3(get)]
    counter: u32,
    #[pyo3(get)]
    replay: bool,
    #[pyo3(get)]
    cycle: bool,
    #[pyo3(get)]
    counter_cycle: u8,
}

#[pymethods]
impl VCDUHeader {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        format!(
            "VCDUHeader(version={}, scid={}, vcid={}, counter={}, replay={}, cycle={}, counter_cycle={})",
            self.version, self.scid, self.vcid, self.counter, self.replay, self.cycle, self.counter_cycle,
        ).to_owned()
    }
}

#[pyclass]
#[derive(Clone, Debug)]
struct Frame {
    #[pyo3(get)]
    header: VCDUHeader,
    #[pyo3(get)]
    rsstate: RSState,
    #[pyo3(get)]
    data: Vec<u8>,
}

#[pymethods]
impl Frame {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        format!(
            "Frame(header={}, rsstate={}, data_len={})",
            self.header.__str__(),
            self.rsstate.__str__(),
            self.data.len(),
        )
        .to_owned()
    }

    #[staticmethod]
    fn decode(dat: &[u8]) -> Option<Self> {
        my::Frame::decode(dat.to_vec()).map(Self::with_frame)
    }
}

impl Frame {
    fn with_frame(frame: my::Frame) -> Self {
        let h = frame.header;
        Self {
            header: VCDUHeader {
                version: h.version,
                scid: h.scid,
                vcid: h.vcid,
                counter: h.counter,
                replay: h.replay,
                cycle: h.cycle,
                counter_cycle: h.counter_cycle,
            },
            rsstate: RSState::NotPerformed,
            data: frame.data,
        }
    }
    fn with_decoded_frame(decoded_frame: my::DecodedFrame) -> Self {
        use my::RSState::{Corrected, NotPerformed, Ok, Uncorrectable};
        let frame = decoded_frame.frame;
        let h = frame.header;
        Self {
            header: VCDUHeader {
                version: h.version,
                scid: h.scid,
                vcid: h.vcid,
                counter: h.counter,
                replay: h.replay,
                cycle: h.cycle,
                counter_cycle: h.counter_cycle,
            },
            rsstate: match decoded_frame.rsstate {
                Ok => RSState::Ok,
                Corrected(_) => RSState::Corrected,
                Uncorrectable(_) => RSState::Uncorrectable,
                NotPerformed => RSState::NotPerformed,
            },
            data: frame.data,
        }
    }
}

#[pyclass]
struct FrameIterator {
    frames: Box<dyn Iterator<Item = my::DecodedFrame> + Send>,
}

#[pymethods]
impl FrameIterator {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<Py<Frame>> {
        match slf.frames.next() {
            Some(decoded_frame) => Py::new(slf.py(), Frame::with_decoded_frame(decoded_frame)).ok(),
            None => None,
        }
    }
}

/// Decode the provided CCSDS Day-Segmented timecode bytes into UTC milliseconds.
///
/// Parameters
/// ----------
/// dat : bytearray
///     Byte array of at least 8 bytes for a CSD timecode. Only the first 8 are used
///     if there are more. Raises a ValueError if there are not enough bytes to decode.
///
/// Raises
/// ------
/// ValueError: If a timecode cannot be created from the provided bytes
#[pyfunction(signature=(dat))]
fn decode_cds_timecode(dat: &[u8]) -> PyResult<u64> {
    let fmt = &timecode::Format::Cds {
        num_day: 2,
        num_submillis: 2,
    };
    match my::timecode::decode(fmt, dat) {
        Ok(tc) => match tc.nanos() {
            Ok(n) => Ok(n / 1_000_000),
            Err(_) => Err(PyValueError::new_err("timecode overflow")),
        },
        Err(_) => Err(PyValueError::new_err("failed to decode timecode")),
    }
}

/// Decode provided bytes representing a CCSDS Unsegmented Timecode as used by the
/// NASA EOS mission (Aqua & Terra) into a UTC timestamp in milliseconds.
///
/// Raises
/// ------
/// ValueError: If a timecode cannot be created from the provided bytes
#[pyfunction(signature=(dat))]
fn decode_eoscuc_timecode(dat: &[u8]) -> PyResult<u64> {
    let fmt = &timecode::Format::Cuc {
        num_coarse: 2,
        num_fine: 2,
        fine_mult: Some(15200.0),
    };
    match my::timecode::decode(fmt, dat) {
        Ok(tc) => match tc.nanos() {
            Ok(n) => Ok(n / 1_000_000),
            Err(_) => Err(PyValueError::new_err("timecode overflow")),
        },
        Err(_) => Err(PyValueError::new_err("failed to decode timecode")),
    }
}

/// Calculate the number of missing packets between cur and last.
///
/// Note, packet sequence counters are per-APID.
#[pyfunction(signature=(cur, last))]
fn missing_packets(cur: u16, last: u16) -> u16 {
    my::missing_packets(cur, last)
}

/// Calculate the number of missing frames between cur and last.
///
/// Note frame sequence counts are per-VCID.
#[pyfunction(signature=(cur, last))]
fn missing_frames(cur: u32, last: u32) -> u32 {
    my::missing_frames(cur, last)
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct PnConfig;

impl PnConfig {
    fn new(config: Option<spacecrafts::PnConfig>) -> Option<Self> {
        config.map(|_| Self {})
    }
}

#[pymethods]
impl PnConfig {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        "PnConfig()".to_string()
    }
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct RSConfig {
    #[pyo3(get)]
    pub interleave: u8,
    #[pyo3(get)]
    pub virtual_fill_length: usize,
    #[pyo3(get)]
    pub num_correctable: u32,
}

#[pymethods]
impl RSConfig {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        format!(
            "RSConfig(interleave={}, virtual_fill_length={}, num_correctable={})",
            self.interleave, self.virtual_fill_length, self.num_correctable
        )
    }
}

impl RSConfig {
    fn new(config: Option<spacecrafts::RSConfig>) -> Option<Self> {
        config.map(|rs| RSConfig {
            interleave: rs.interleave,
            virtual_fill_length: rs.virtual_fill_length,
            num_correctable: rs.num_correctable,
        })
    }
}

#[pyclass]
#[derive(Clone, Debug)]
pub struct FramingConfig {
    #[pyo3(get)]
    pub length: usize,
    #[pyo3(get)]
    pub insert_zone_length: usize,
    #[pyo3(get)]
    pub trailer_length: usize,
    #[pyo3(get)]
    pub pseudo_noise: Option<PnConfig>,
    #[pyo3(get)]
    pub reed_solomon: Option<RSConfig>,
}

impl FramingConfig {
    fn new(config: spacecrafts::FramingConfig) -> Self {
        Self {
            length: config.length,
            insert_zone_length: config.insert_zone_length,
            trailer_length: config.trailer_length,
            pseudo_noise: PnConfig::new(config.pseudo_noise),
            reed_solomon: RSConfig::new(config.reed_solomon),
        }
    }
}

#[pymethods]
impl FramingConfig {
    fn __repr__(&self) -> String {
        self.__str__()
    }
    fn __str__(&self) -> String {
        let pn = match &self.pseudo_noise {
            Some(pn) => pn.__str__(),
            None => "None".to_string(),
        };
        let rs = match &self.reed_solomon {
            Some(rs) => rs.__str__(),
            None => "None".to_string(),
        };
        format!("FramingConfig(length={}, insert_zone_length={}, trailer_length={}, pseudo_noise={}, reed_solomon={})",
        self.length, self.insert_zone_length, self.trailer_length, pn, rs).to_string()
    }

    /// Return the computed length of a CADU block, i.e., CADU length - ASM length, from
    /// our config.
    pub fn codeblock_len(&self) -> usize {
        match &self.reed_solomon {
            Some(rs) => self.length + 2 * rs.num_correctable as usize * rs.interleave as usize,
            None => self.length,
        }
    }
}

/// Lookup the FramingConfig for a spacecraft.
///
/// This makes use of a spacecraftsdb formatted database file. See the releases at
/// https://github.com/bmflynn/spacecraftsdb to download a database file.
///
/// Parameters
/// ----------
/// scid : int
///     The spacecraft identifier for a spacecraft.
///
/// path : str, optional
///     Local path to a specific spacecraftsdb database file. If not provided this will
///     attempt to load the database from ./spacecraftsdb.json,
///     $XDG_DATA_HOME/spacecraftsdb/spacecraftsdb.json, ~/.spacecraftsdb.json.
///
/// Returns
/// -------
/// FramingConfig or None
///     The configuration for the specified spacecraft if available, otherwise `None`
///
/// Raises
/// ------
/// ValueError:
///     If the spacecraft database cannot be loaded.
#[pyfunction]
#[pyo3(signature = (scid, path=None))]
fn framing_config(scid: u16, path: Option<&str>) -> PyResult<Option<FramingConfig>> {
    let db = match path {
        Some(path) => spacecrafts::DB::with_path(path),
        None => spacecrafts::DB::new(),
    };

    let db = match db {
        Ok(db) => db,
        Err(err) => {
            return Err(PyValueError::new_err(format!(
                "failed to init spacecraft db: {err}"
            )))
        }
    };

    match db.find(scid) {
        Some(spacecraft) => Ok(Some(FramingConfig::new(spacecraft.framing_config))),
        None => Ok(None),
    }
}

/// ccsds
///
/// Python wrapper for the [ccsds](https://github.com/bmflynn/ccsds) Rust crate
/// providing decode capabilities for frames (sync, RS, pn, etc ...) and spacepackets.
#[pymodule]
#[pyo3(name = "ccsds")]
fn ccsdspy(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(decode_packets, m)?)?;
    m.add_function(wrap_pyfunction!(decode_packet_groups, m)?)?;
    m.add_class::<Packet>()?;
    m.add_class::<PacketGroup>()?;
    m.add_class::<DecodedPacket>()?;
    m.add_class::<PrimaryHeader>()?;
    m.add_class::<RSState>()?;

    m.add_function(wrap_pyfunction!(rs_correct_codeblock, m)?)?;
    m.add_function(wrap_pyfunction!(pndecode, m)?)?;
    m.add_function(wrap_pyfunction!(synchronized_blocks, m)?)?;
    m.add_class::<Frame>()?;
    m.add_class::<VCDUHeader>()?;

    m.add_function(wrap_pyfunction!(decode_cds_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_eoscuc_timecode, m)?)?;

    m.add_function(wrap_pyfunction!(missing_packets, m)?)?;
    m.add_function(wrap_pyfunction!(missing_frames, m)?)?;
    m.add_function(wrap_pyfunction!(framing_config, m)?)?;

    m.add("ASM", my::ASM)?;

    Ok(())
}
