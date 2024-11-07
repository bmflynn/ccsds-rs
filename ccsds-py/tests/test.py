from pathlib import Path

import ccsds
import pytest


def fixture_path(name: str) -> str:
    return str(Path(__file__).parent.parent.parent / "tests" / "fixtures" / name)


@pytest.mark.parametrize(
    "path,block_size,scid,vcid,counter",
    [
        pytest.param(
            fixture_path("jpss2_block.dat"), 1275, 177, 16, 12077315, id="jpss2_I5"
        ),
        pytest.param(
            fixture_path("snpp_block.dat"), 1020, 157, 16, 9842876, id="snpp_I4"
        ),
    ],
)
def test_pipeline(path, block_size, scid, vcid, counter):
    blocks = list(ccsds.synchronized_blocks(path, block_size))
    assert len(blocks) == 1, "execpected single block from pipelie test fixture"

    # skipping RS
    frame = ccsds.Frame.decode(ccsds.pndecode(blocks[0]))

    assert frame.header.scid == scid
    assert frame.header.vcid == vcid
    assert frame.header.counter == counter
