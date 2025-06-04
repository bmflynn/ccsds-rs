use crate::{
    framing::{packets::FramedPacketIter, Frame},
    spacepacket::Packet,
};

/// Decode frame data into spacepackets.
///
/// Packets are decoded in the order in which they are received, per VCID.
///
/// Packet data may be dropped/lost in the following cases:
///
/// * Not enough data left to construct an entire frame.
/// * Not enough data left to construct an entire packet.
/// * Not enough data within the frame to construct a packet primary header.
/// * Frame received with that contains errors ([Integrity::Uncorrectable](crate::framing),
///   [Integrity::NotCorrected](crate::framing))
/// * Invalid MPDU first header pointer value
/// * Discontinuity in the frame counter from the current frame to the previous frame of the same
///   VCID.
///
/// # Arguments
/// * `izone_length` is the number of bytes used for the Insert Zone, i.e., extra data inserted
/// between the transfer frame header and data field. This data is not currently used to decode the
/// packet data, but must be accounted for when computing offsets to contained data.
///
/// * `trailer_length` is the number of bytes of data after the transfer frame data section but
/// before any Reed Solomon bytes (if used). This is typically referred to as the Operational
/// Control Field.
///
/// # Example
/// ```
/// use ccsds::framing::{Frame, packet_decoder};
/// use ccsds::spacepacket::Packet;
///
/// let frames = vec![Frame::decode(vec![0u8; 1020]).unwrap()];
/// let packets: Vec<Packet> = packet_decoder(frames.into_iter(), 0, 0).collect();
/// ```
///
pub fn packet_decoder<I>(
    frames: I,
    izone_length: usize,
    trailer_length: usize,
) -> impl Iterator<Item = Packet>
where
    I: Iterator<Item = Frame>,
{
    let iter = FramedPacketIter::new(frames, izone_length, trailer_length);

    iter
}
