use crate::{
    framing::{packets::PacketDemux, Frame},
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
    let ocf = trailer_length == 4 || trailer_length == 6;
    let fec = trailer_length == 2 || trailer_length == 6;
    let iter = PacketDemux::new(frames).with_defaults(izone_length, false, ocf, fec);

    iter
}
