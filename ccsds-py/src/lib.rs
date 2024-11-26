use std::{fs::File, io::Read};

use ccsds::{
    spacepacket::{collect_groups, decode_packets, Packet, PacketGroup, PrimaryHeader},
    timecode::Format,
};
use pyo3::prelude::*;

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
/// Returns
/// -------
/// Iterator of `Packet`s
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
        //match slf.groups.next() {
        //    Some(group) => Py::new(slf.py(), PacketGroup::from_group(group)).ok(),
        //    None => None,
        //}
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
fn decode_packet_groups(path: &str) -> PyResult<PacketGroupIter> {
    let file: Box<dyn Read + Send> = Box::new(File::open(path)?);
    let packets = decode_packets(file).filter_map(Result::ok);
    let groups = Box::new(collect_groups(packets).filter_map(Result::ok));
    Ok(PacketGroupIter { groups })
}

#[pyclass(frozen)]
struct Timecode {
    epoch: hifitime::Epoch,
}

#[pymethods]
impl Timecode {
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
    /// Timecode
    #[staticmethod]
    fn decode_cds(num_day: usize, num_submillis: usize, buf: &[u8]) -> PyResult<Self> {
        let fmt = ccsds::timecode::Format::Cds {
            num_day,
            num_submillis,
        };
        Ok(Timecode {
            epoch: ccsds::timecode::decode(&fmt, buf)?,
        })
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
    /// Timecode
    #[staticmethod]
    #[pyo3(signature=(num_coarse, num_fine, buf, fine_mult=None))]
    fn decode_cuc(
        num_coarse: usize,
        num_fine: usize,
        buf: &[u8],
        fine_mult: Option<f32>,
    ) -> PyResult<Self> {
        let fmt = Format::Cuc {
            num_coarse,
            num_fine,
            fine_mult,
        };
        Ok(Timecode {
            epoch: ccsds::timecode::decode(&fmt, buf)?,
        })
    }

    /// Decode JPSS CDS timecode
    #[staticmethod]
    fn decode_jpss(buf: &[u8]) -> PyResult<Self> {
        Self::decode_cds(2, 2, buf)
    }

    /// Decode NASA EOS (Aqua/Terra) Telemetry CUC timecode
    #[staticmethod]
    fn decode_eos(buf: &[u8]) -> PyResult<Self> {
        Self::decode_cuc(2, 4, buf, Some(15200.0))
    }

    fn __str__(&self) -> String {
        self.epoch.to_string()
    }

    /// Returns seconds since Jan 1, 1970
    fn unix_seconds(&self) -> f64 {
        self.epoch.to_unix_seconds()
    }

    fn epoch(&self) -> hifitime::Epoch {
        self.epoch
    }

    /// Extract timecode  as a `datetime.datetime`
    fn datetime<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let datetime = py.import_bound("datetime")?;
        let utc = datetime.getattr("timezone")?.getattr("utc")?;
        datetime
            .getattr("datetime")?
            .getattr("fromtimestamp")?
            .call1((self.epoch.to_unix_seconds(), utc))
    }
}

#[pymodule]
#[pyo3(name = "ccsds")]
#[pyo3(module = "ccsds")]
fn ccsdspy(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_decode_packets, m)?)?;
    m.add_function(wrap_pyfunction!(decode_packet_groups, m)?)?;

    m.add_class::<Packet>()?;
    m.add_class::<PrimaryHeader>()?;
    m.add_class::<PacketGroup>()?;
    m.add_class::<Timecode>()?;

    Ok(())
}
