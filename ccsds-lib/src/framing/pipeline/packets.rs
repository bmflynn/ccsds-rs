use crate::{
    framing::{packets::FramedPacketIter, Frame},
    spacepacket::Packet,
};

pub fn packet_decoder<I>(
    frames: I,
    izone_length: usize,
    trailer_length: usize,
) -> impl Iterator<Item = Packet> + Send + 'static
where
    I: Iterator<Item = Frame> + Send + 'static,
{
    let iter = FramedPacketIter::new(frames, izone_length, trailer_length);

    iter
}
