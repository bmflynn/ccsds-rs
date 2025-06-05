# ccsds Python bindings

[![Actions Status](https://img.shields.io/github/actions/workflow/status/bmflynn/ccsdspy/CI.yml?branch=main&logo=github&style=flat-square)](https://github.com/bmflynn/ccsdspy/actions)

> [!WARNING]
> This project is very much in development, and the API is very likely to change in ways that will 
> things. If you have comments or suggestions regarding the API feel free to file an issue.

`ccsds` provides Python bindnigs for the [ccsds](https://github.com/bmflynn/ccsds-rs/lib)
Rust bindings using [pyo3](https://pyo3.rs).

## Extracting Frames and Packets

```python
from ccsds import PacketExtractor, SyncOpts, decode_frames

extractor = PacketExtractor()

# Path to raw unsynchronized CADU file
input = "..."
frames = decode_frames(input, SyncOpts(1020))

for frame in frames:
    zult = extractor.handle(frame)
    if not zult:
        continue  # no packets could be produced
    if zult.drop:
        print(f"frame dropped reason={zult.reason}")
        continue
    for packet in zult.packets:
        print(packet)
```


## Related

* [spacecraftsdb](https://github.com/bmflynn/spacecraftsdb): JSON Spacecraft metadata database
* [spacecrafts-rs](https://github.com/bmflynn/spacecrafts-rs): Rust crate interfacing with `spacecraftsdb`
* [ccsds-rs](https://github.com/bmflynn/ccsds-rs): Rust crate for CCSDS spacecraft frame & spacepacket decoding
