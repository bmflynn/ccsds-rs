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
    @staticmethod
    def decode_cds(num_day: int, num_submillis: int, buf: bytes) -> "Timecode": ...
    @staticmethod
    def decode_cuc(
        num_coarse: int, num_fine: int, buf: bytes, fine_mult: float | None = None
    ) -> "Timecode": ...
    @staticmethod
    def decode_eos(buf: bytes) -> "Timecode": ...
    @staticmethod
    def decode_jpss(buf: bytes) -> "Timecode": ...
    def datetime(self) -> datetime: ...
    def unix_seconds(self) -> float: ...
    def epoch(self) -> "hifitime.Epoch": ...
