import hashlib
from pathlib import Path

import ccsdspy


def fixture_path(name: str) -> str:
    return str(Path(__file__).parent / "fixtures" / name)


def test_read_framed_packets():
    packet_iter = ccsdspy.read_framed_packets(
        fixture_path("snpp_synchronized_cadus.dat"), 157, 4
    )

    csum = hashlib.md5()
    for p in packet_iter:
        csum.update(bytes(p.data))
    assert (
        csum.hexdigest() == "5e11051d86c46ddc3500904c99bbe978"
    ), "packet output file does not match fixture checksum"
