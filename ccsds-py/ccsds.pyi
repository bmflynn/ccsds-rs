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

def decode_packets(path: str) -> typing.Iterable[Packet]: ...
def decode_packet_groups(path: str) -> typing.Iterable[PacketGroup]: ...

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

class SyncOpts:
    def __init__(self, length: int): ...
    def with_asm(self, asm: bytes) -> "SyncOpts": ...

class RsOpts:
    def __init__(self, interleave: int): ...
    def with_num_threads(self, num_threads: 0) -> "RsOpts": ...
    def with_virtual_fill(self, num: 0) -> "RsOpts": ...
    def with_correction(self, enabled: bool) -> "RsOpts": ...
    def with_detection(self, enabled: bool) -> "RsOpts": ...
    def with_buffer_size(self, size: int) -> "RsOpts": ...

class Integrity:
    Ok: int
    Corrected: int
    Uncorrectable: int
    NotCorrected: int
    Skipped: int
    Failed: int

class VCDUHeader:
    scid: int
    vcid: int
    version: 0
    counter: int
    replay: bool

    def __init__(
        self,
        scid: int,
        vcid: int,
        version: int = 0,
        counter: int = 0,
        replay: bool = False,
    ): ...

class MPDU:
    first_header: int
    data: bytes

class Frame:
    header: VCDUHeader
    missing: int
    integrity: Integrity | None
    data: bytes

    def __init__(
        self,
        header: VCDUHeader,
        missing: int | None = None,
        integrity: Integrity | None = None,
        data: bytes | None = None,
    ): ...
    def mpdu(self) -> MPDU | None: ...
    def is_fill(self) -> bool: ...

def decode_framed_packets(
    uri: str,
    sync: SyncOpts,
    pn: bool = False,
    rs: RsOpts | None = None,
    izone_length: int = 0,
    trailer_length: int = 0,
) -> typing.Iterable[Packet]: ...

def decode_frames(
    uri: str,
    sync: SyncOpts,
    pn: bool = False,
    rs: RsOpts | None = None,
) -> typing.Iterable[Frame]: ...

class ExtractorResult:
    packets: typing.Iterable[Packet]
    drop: bool
    reason: str

class PacketExtractor:
    def __init__(self, izone_length: int = 0, trailer_length: int = 0): ...
    def handle(self, frame: Frame) -> "ExtractorResult": ...