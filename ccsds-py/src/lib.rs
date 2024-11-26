use std::{fs::File, io::Read};

use ccsds::{
    spacepacket::{collect_groups, decode_packets, Packet, PacketGroup, PrimaryHeader},
    timecode::Format as TimecodeFormat,
};
use pyo3::prelude::*;

// FIXME: Remove "unsendable"
//        This may require some usage of Arc or something
#[pyclass(unsendable)]
struct PacketIter {
    packets: Box<dyn Iterator<Item = Packet> + Send>,
}

#[pymethods]
impl PacketIter {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<Packet> {
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
fn py_decode_packets(path: &str) -> PyResult<PacketIter> {
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets: Box<dyn Iterator<Item = Packet> + Send + 'static> =
        Box::new(decode_packets(file).filter_map(|z| z.ok()));

    Ok(PacketIter { packets })
}

#[pyclass(unsendable)]
struct PacketGroupIter {
    groups: Box<dyn Iterator<Item = PacketGroup> + Send>,
}

#[pymethods]
impl PacketGroupIter {
    fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<Self>) -> Option<PacketGroup> {
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
fn decode_packet_groups(path: &str) -> PyResult<PacketGroupIter> {
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets = decode_packets(file).filter_map(Result::ok);
    let groups = Box::new(collect_groups(packets).filter_map(Result::ok));
    Ok(PacketGroupIter { groups })
}

#[pyclass(frozen)]
struct Timecode {
    #[pyo3(get)]
    epoch: hifitime::Epoch,
}

#[pymethods]
impl Timecode {
    fn __repr__(&self) -> String {
        self.__str__()
    }

    // str rep that is loadable by datetime.fromisoformat
    fn __str__(&self) -> String {
        self.epoch.to_string()
    }

    /// Returns seconds since Jan 1, 1970
    ///
    /// Returns:
    ///     A hifitime.Epoch instance representing this timecode.
    fn unix_seconds(&self) -> f64 {
        self.epoch.to_unix_seconds()
    }

    /// Extract timecode as a `datetime.datetime`.
    ///
    /// Returns:
    ///     A datetime with its tzinfo set to `datetime.timezone.utc`.
    ///
    ///     Note, that datetime does not support time anything more than microsecond precision
    ///     and any nanoseconds present are silently dropped.
    fn datetime<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let datetime = py.import_bound("datetime")?;
        let utc = datetime.getattr("timezone")?.getattr("utc")?;
        datetime
            .getattr("datetime")?
            .getattr("fromtimestamp")?
            .call1((self.epoch.to_unix_seconds(), utc))
    }
}

/// Decode the provided data into a `Timecode`.
///
/// Args:
///     format:
///         A Format instance specifying the timecode parameters used for decoding
///     buf:
///         Data to decode. Must be at least as long as the format requires. decoding
///         will always start at index 0.
///
/// Returns:
///     Timecode
///
/// Raises:
///     ValueError: If `buf` cannot meet the format requirements
#[pyfunction]
fn decode_timecode(format: TimecodeFormat, buf: &[u8]) -> PyResult<Timecode> {
    Ok(Timecode {
        epoch: ccsds::timecode::decode(&format, buf)?,
    })
}

/// Decode NASA EOS telemetry CUC timecode
///
/// See decode_timecode
#[pyfunction(name = "_decode_eos_timecode")]
fn decode_eos_timecode(buf: &[u8]) -> PyResult<Timecode> {
    let format = TimecodeFormat::Cuc {
        num_coarse: 2,
        num_fine: 4,
        fine_mult: Some(15200.0),
    };
    Ok(Timecode {
        epoch: ccsds::timecode::decode(&format, buf)?,
    })
}

/// Decode JPSS CDS timecode.
///
/// See decode_timecode
#[pyfunction(name = "_decode_jpss_timecode")]
fn decode_jpss_timecode(buf: &[u8]) -> PyResult<Timecode> {
    let format = TimecodeFormat::Cds {
        num_day: 2,
        num_submillis: 2,
    };
    Ok(Timecode {
        epoch: ccsds::timecode::decode(&format, buf)?,
    })
}

#[pymodule]
#[pyo3(name = "ccsds")]
#[pyo3(module = "ccsds")]
fn ccsdspy(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_decode_packets, m)?)?;
    m.add_function(wrap_pyfunction!(decode_packet_groups, m)?)?;
    m.add_function(wrap_pyfunction!(decode_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_eos_timecode, m)?)?;
    m.add_function(wrap_pyfunction!(decode_jpss_timecode, m)?)?;

    m.add_class::<Packet>()?;
    m.add_class::<PrimaryHeader>()?;
    m.add_class::<PacketGroup>()?;
    m.add_class::<Timecode>()?;
    m.add_class::<TimecodeFormat>()?;

    Ok(())
}
