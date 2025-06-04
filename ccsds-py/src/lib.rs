use std::{fs::File, io::Read};

use ccsds::{
    framing::{Block, Frame, Integrity, Loc, Pipeline, RsOpts, SyncOpts, VCDUHeader, MPDU},
    spacepacket::{collect_groups, decode_packets, Packet, PacketGroup, PrimaryHeader},
    timecode::Format as TimecodeFormat,
};
use pyo3::prelude::*;

macro_rules! create_iter {
    ($name: ident, $type: ident) => {
        #[pyclass(unsendable)]
        pub struct $name {
            iter: Box<dyn Iterator<Item = $type>>,
        }
        #[pymethods]
        impl $name {
            fn __iter__(slf: PyRef<Self>) -> PyRef<Self> {
                slf
            }

            fn __next__(mut slf: PyRefMut<Self>) -> Option<$type> {
                slf.iter.next()
            }
        }
    };
}

create_iter!(PacketIter, Packet);
create_iter!(PacketGroupIter, PacketGroup);
create_iter!(BlockIter, Block);
create_iter!(FrameIter, Frame);

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

    Ok(PacketIter { iter: packets })
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
    Ok(PacketGroupIter { iter: groups })
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
        let datetime = py.import("datetime")?;
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

/// Byte-align and locate blocks of data in an input bit stream.
///
/// Args:
///     uri:
///         URI for a supported input bit stream. The bit stream need not be synchronized
///         or byte-aligned.
///     sync:
///         Options for the synchronization process
///
/// Returns:
///     An iterator of byte-aligned blocks of data located in the bit stream
#[pyfunction]
fn synchronize(uri: &str, opts: SyncOpts) -> PyResult<BlockIter> {
    let reader = File::open(uri)?;

    Ok(BlockIter {
        iter: Box::new(ccsds::framing::synchronize(reader, opts)),
    })
}

/// Decode the input stream indicated by `uri` into frames.
///
/// Args:
///     uri:
///         URI for a supported input bit stream. The bit stream need not be synchronized
///         or byte-aligned.
///     sync:
///         Options for the synchronization process
///     pn:
///         If `True` derandominze data from the input stream before framing
///     rs:
///         Options for the ReedSolomon process. If `None` no RS is performed and all frames
///         integrity will indicate `Integrity:Skipped`
///
/// Returns:
///     An iterable of Frames
#[pyfunction(signature=(uri, sync, pn=false, rs=None))]
fn decode_frames(uri: &str, sync: SyncOpts, pn: bool, rs: Option<RsOpts>) -> PyResult<FrameIter> {
    let mut pipeline = Pipeline::new(sync.length);

    if !pn {
        pipeline = pipeline.without_derandomization();
    }

    if let Some(rs) = rs {
        pipeline = pipeline.with_rs(rs);
    }
    let file = File::open(uri)?;

    Ok(FrameIter {
        iter: Box::new(pipeline.start(file)),
    })
}

/// Decode the input stream indicated by `uri` into packets.
///
/// Packets are produced in the order in which they appear in the stream.
///
/// Args:
///     uri:
///         URI for a supported input bit stream. The bit stream need not be synchronized
///         or byte-aligned.
///     sync:
///         Options for the synchronization process
///     pn:
///         If `True` derandominze data from the input stream before framing
///     rs:
///         Options for the ReedSolomon process. If `None` no RS is performed and all frames
///         integrity will indicate `Integrity:Skipped`
///     izone_length:
///         Number of bytes of insert zone, if any.
///     trailer_length:
///         Number of bytes of trailer(OCF) data, if any.
///
/// Returns:
///     An iterable of Packets
#[pyfunction(signature=(uri, sync, pn=false, rs=None, izone_length=None, trailer_length=None))]
fn decode_framed_packets(
    uri: &str,
    sync: SyncOpts,
    pn: bool,
    rs: Option<RsOpts>,
    izone_length: Option<usize>,
    trailer_length: Option<usize>,
) -> PyResult<PacketIter> {
    let mut pipeline = Pipeline::new(sync.length);

    if !pn {
        pipeline = pipeline.without_derandomization();
    }

    if let Some(rs) = rs {
        pipeline = pipeline.with_rs(rs);
    }
    let file = File::open(uri)?;

    let packets = ccsds::framing::packet_decoder(
        pipeline.start(file),
        izone_length.unwrap_or_default(),
        trailer_length.unwrap_or_default(),
    );
    Ok(PacketIter {
        iter: Box::new(packets),
    })
}

#[pymodule]
#[pyo3(name = "ccsds")]
#[pyo3(module = "ccsds")]
fn ccsdspy(root: &Bound<'_, PyModule>) -> PyResult<()> {
    root.add_function(wrap_pyfunction!(py_decode_packets, root)?)?;
    root.add_function(wrap_pyfunction!(decode_packet_groups, root)?)?;
    root.add_function(wrap_pyfunction!(decode_timecode, root)?)?;
    root.add_function(wrap_pyfunction!(decode_eos_timecode, root)?)?;
    root.add_function(wrap_pyfunction!(decode_jpss_timecode, root)?)?;

    root.add_function(wrap_pyfunction!(synchronize, root)?)?;
    root.add_function(wrap_pyfunction!(decode_frames, root)?)?;
    root.add_function(wrap_pyfunction!(decode_framed_packets, root)?)?;

    root.add_class::<Packet>()?;
    root.add_class::<PrimaryHeader>()?;
    root.add_class::<PacketGroup>()?;
    root.add_class::<Timecode>()?;
    root.add_class::<TimecodeFormat>()?;
    root.add_class::<Frame>()?;
    root.add_class::<MPDU>()?;
    root.add_class::<VCDUHeader>()?;
    root.add_class::<Block>()?;
    root.add_class::<Loc>()?;
    root.add_class::<SyncOpts>()?;
    root.add_class::<RsOpts>()?;
    root.add_class::<Integrity>()?;

    Ok(())
}
