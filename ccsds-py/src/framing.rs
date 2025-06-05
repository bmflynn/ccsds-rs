use std::fs::File;

use ccsds::{framing::{Block, Derandomizer, Frame, Integrity, Loc, Pipeline, RsOpts, SyncOpts, VCDUHeader, MPDU}, spacepacket::Packet};
use pyo3::prelude::*;

use crate::{BlockIter, FrameIter, PacketIter};


#[pyfunction]
fn derandomize(mut block: Block) -> Block {
    block.data = ccsds::framing::DefaultDerandomizer::default().derandomize(&block.data);
    block
}

#[pyclass(get_all)]
struct ExtractResult {
    pub packets: Vec<Packet>,
    pub drop: bool,
    pub reason: String,
}


/// Extracts packets from frames.
/// 
/// A cache is maintained of partial packets data that have not yet been decoded into
/// into valid [Packet]s. As frames are processed, the cache is updated with new data
/// and packets are extracted from the cache when enough data is available to construct
/// a complete packet. The cache is keyed by VCID, so packets from different VCIDs are
/// not mixed together. The cache is cleared when a frame is received that has a
/// discontinuity in the frame counter, or when a frame is received that has an
/// integrity error (e.g., [Integrity::Uncorrectable](crate::framing) or
/// [Integrity::NotCorrected](crate::framing)).
#[pyclass]
#[pyo3(name = "PacketExtractor")]
#[derive(Debug, Clone)]
struct PacketExtractorAdapter{
    extractor: ccsds::framing::PacketExtractor,
}

#[pymethods]
impl PacketExtractorAdapter {

    #[new]
    #[pyo3(signature=(izone_length=None, trailer_length=None))]
    pub fn new(
        izone_length: Option<usize>,
        trailer_length: Option<usize>,
    ) -> Self {
        PacketExtractorAdapter {
            extractor: ccsds::framing::PacketExtractor::new(
                izone_length.unwrap_or_default(),
                trailer_length.unwrap_or_default(),
            ),
        }
    }

    /// Handle a single frame by updating the internal cache and extracting all packets that can 
    /// become complete from the current cache state.
    /// 
    /// Args:
    ///     frame: The frame to process.
    /// 
    /// Returns:
    ///     A result containing all packets that were extracted, if any, and a flag indicating if
    ///     the frame was dropped due to an error or data discontinuity. If the frame's data was
    ///     processed successfully but no packets were extracted the `drop` flag will be `false`
    ///     and the `packets` data will be empty.
    pub fn handle(&mut self, frame: Frame) -> Option<ExtractResult> {
        use ccsds::framing::ExtractResult as ER;
        match self.extractor.handle(&frame) {
            ER::Packets(packets) => {
                Some(ExtractResult{packets, drop: false, reason: String::new()})
            },
            ER::Drop(reason) => {
                Some(ExtractResult{packets: Vec::new(), drop: true, reason})
            },
            ER::None => None,
        }
    }
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


/// Decode the input stream indicated by `uri` into frames. The decode process includes synchronization,
/// and can therefore take some time to scan through the input stream before the producing
/// the first frame.
///
/// Args:
///     uri:
///         URI for a supported input bit stream. The bit stream need not be synchronized
///         or byte-aligned.
///     sync:
///         Options for the synchronization process
///     pn:
///         If `False` disable data derandomization.
///     rs:
///         Options for the ReedSolomon process. If `None` no RS is performed and all frames
///         integrity will indicate `Integrity:Skipped`
///
/// Returns:
///     An iterable of Frames
#[pyfunction(signature=(uri, sync, pn=true, rs=None))]
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
#[pyfunction(signature=(uri, sync, pn=false, rs=None, izone_length=0, trailer_length=0))]
fn decode_framed_packets(
    uri: &str,
    sync: SyncOpts,
    pn: bool,
    rs: Option<RsOpts>,
    izone_length: usize,
    trailer_length: usize,
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
        izone_length,
        trailer_length,
    );
    Ok(PacketIter {
        iter: Box::new(packets),
    })
}

pub(crate) fn register(root: &Bound<'_, PyModule>) -> PyResult<()> {

    root.add_function(wrap_pyfunction!(derandomize, root)?)?;
    root.add_function(wrap_pyfunction!(synchronize, root)?)?;
    root.add_function(wrap_pyfunction!(decode_frames, root)?)?;
    root.add_function(wrap_pyfunction!(decode_framed_packets, root)?)?;

    root.add_class::<PacketExtractorAdapter>()?;
    root.add_class::<ExtractResult>()?;
    root.add_class::<Frame>()?;
    root.add_class::<FrameIter>()?;
    root.add_class::<MPDU>()?;
    root.add_class::<VCDUHeader>()?;
    root.add_class::<Block>()?;
    root.add_class::<BlockIter>()?;
    root.add_class::<Loc>()?;
    root.add_class::<SyncOpts>()?;
    root.add_class::<RsOpts>()?;
    root.add_class::<Integrity>()?;
    
    Ok(())
}