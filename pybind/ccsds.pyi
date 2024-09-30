import enum
import typing

ASM: list[int] 

class RSState(enum.Enum):
    OK = 0
    Corrected = 1
    Uncorrected = 2
    NotPerformed = 3

class VCDUHeader:
    version: int
    scid: int
    vcid: int
    counter: int
    replay: bool
    cycle: bool
    counter_cycle: int

class Frame:
    header: VCDUHeader
    rsstate: RSState
    data: bytes

    @staticmethod
    def decode(dat: list[int] | bytes) -> 'Frame': ...

class PrimaryHeader:
    version: int
    type_flag: int
    has_secondary_header: bool
    apid: int
    sequence_flags: int
    sequence_id: int
    len_minus1: int

    @classmethod
    def decode(cls, dat: bytes) -> PrimaryHeader: ...

class Packet:
    header: PrimaryHeader
    data: bytes

    @classmethod
    def decode(cls, dat: bytes) -> Packet: ...

class DecodedPacket:
    scid: int
    vcid: int
    packet: Packet

def synchronized_blocks(source: str, block_size: int, asm: list[int] | bytes | None = None) -> typing.Iterator[bytes]: ...

def pndecode(dat: list[int] | bytes) -> bytes: ...

def decode_packets(source: str) -> typing.Iterable[Packet]: ...
def decode_frames(
    source: str, frame_len: int, interleave: int
) -> typing.Iterable[Frame]: ...
def decode_framed_packets(
    source: str,
    scid: int,
    frame_len: int,
    izone_len: int = 0,
    trailer_len: int = 0,
    interleave: int | None = None,
) -> typing.Iterable[DecodedPacket]: ...
def decode_cdc_timecode(dat: bytes) -> int: ...
def decode_eoscuc_timecode(dat: bytes) -> int: ...
def missing_packets(cur: int, last: int) -> int: ...
def missing_frames(cur: int, last: int) -> int: ...
