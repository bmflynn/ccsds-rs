from datetime import datetime, timezone
import ccsds

import pytest

header_len = 6 


@pytest.fixture
def packet_data():
    return bytes(
        [0xD, 0x59, 0xC0, 0x01, 0x0, 0x8, 0x52, 0xC0, 0x0, 0x0, 0x0, 0xA7, 0x0, 0xDB, 0xFF]
    )


def test_packet(packet_data):
    dat = packet_data
    pkt = ccsds.Packet(dat)

    assert pkt.header.apid == 1369
    assert pkt.header.len_minus1 == len(dat) - header_len - 1
    assert len(pkt.data) == len(dat)
    assert len(pkt.user_data) == len(dat) - header_len
    assert pkt.data[:header_len] == dat[:header_len]
    assert pkt.user_data == dat[header_len:]



def test_timecode(packet_data):
    dat = packet_data
    tc = ccsds._decode_jpss_timecode(dat[header_len:])
    assert str(tc) == "2016-01-01T00:00:00.167219000 UTC"
    assert tc.datetime() == datetime(2016, 1, 1, 0, 0, 0, 167219, tzinfo=timezone.utc)
    assert tc.unix_seconds() == 1451606400.167219
