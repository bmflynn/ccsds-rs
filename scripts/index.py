#!/usr/bin/env python3
import argparse
from pathlib import Path

from edosl0util import stream

parser = argparse.ArgumentParser()
parser.add_argument("dat", type=Path)
args = parser.parse_args()

with open(f"{args.dat}.idx", "wt") as fp:
    for group in stream.collect_groups(stream.jpss_packet_stream(open(args.dat, "rb"))):
        dt = group[0].stamp
        for pkt in group:
            fp.write(f"{pkt.apid}, {dt:%Y-%m-%dT%H:%M:%S.%fZ}, {pkt.seqid}\n")
