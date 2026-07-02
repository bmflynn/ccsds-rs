import typing
from datetime import datetime

import hifitime

class PrimaryHeader:
    version: int
    type_flag: int
    has_secondary_header: bool
    apid: int
    sequence_flags: int
    sequence_id: int
    len_minus1: int

class Packet:
    data: bytes
    user_data: bytes
    header: PrimaryHeader

    def __init__(self, buf: bytes): ...

class PacketGroup:
    apid: int
    packets: typing.Iterable[Packet]
    complete: bool
    have_missing: bool

def decode_packets(path: str) -> typing.Iterable[Packet]:
    """Decode packets from a packet file on disk.

    Arguments:
        path: Path to file on disk

    Returns:
        Iterable of ccsds.Packet
    """

def decode_packet_groups(path: str) -> typing.Iterable[PacketGroup]: ...
    """Decode packets that may be groupped from a packet file on disk.

    Arguments:
        path: Path to file on disk

    Returns:
        Iterable of ccsds.PacketGroup
    """

class Timecode:
    epoch: hifitime.Epoch

    def datetime(self) -> datetime: ...
    def unix_seconds(self) -> float: ...

class Format:
    class Cds:
        num_day: int
        num_submillis: int

    class Cuc:
        num_coarse: int
        num_fine: int
        fine_mult: float | None

def decode_timecode(format: Format, buf: bytes) -> Timecode: ...
def _decode_jpss_timecode(buf: bytes) -> Timecode: ...
def _decode_eos_timecode(buf: bytes) -> Timecode: ...

class FramingConfig:
    """ Specified framing configuration. Intended to use as a builder class. See the with_* methods.
    """
    # Cadu length not including ASM
    cadu_length: int
    # Do not perform denoising
    without_pn: bool
    # Reed-solomon interleave. If not specified the input stream must contain no RS 
    # parity bytes.
    rs: int | None
    # Perform RS correction. If False, error detection may still be performed.
    rs_correct: bool
    # Detect RS errors, but do not correct
    rs_detect: bool
    # Number of RS virtual fill bytes
    rs_virtualfill: int

    def __init__(self, cadu_length: int):
        """ Create a new config using the specified cadu_length.

        Arguments:
            cadu_length: Length of a CADU, not including the ASM
        """


def derandomize(data: bytes) -> bytes:
    """ Remove pseudo-random noise from data.
    """

def decode_frames(input: str, config: FramingConfig) -> typing.Iterable[Frame]:
    """ Decode a raw bitstream to frames according to config.

    Arguments:
        input: 
            May be tcp://<addr>:<port>, ipc://<path>, - (for stdin), or a file path.
        config:
            The specifics of the decode process.

    Returns:
        An interable of frames.
    """
