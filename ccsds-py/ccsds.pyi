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
