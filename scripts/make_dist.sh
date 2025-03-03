#!/bin/bash
set -e

target=x86_64-unknown-linux-musl

cargo build -p ccsds-cmd --target=${target} -r

cwd=${PWD}
workdir=$(mktemp -d)
version=$(cd ccsds-cmd && cargo read-manifest | jq -r .version)
trap "rm -rf ${workdir}" EXIT
target=x86_64-unknown-linux-musl
distdir=$workdir/$target
mkdir $distdir
rsync -av target/x86_64-unknown-linux-musl/release/ccsds $distdir/
rsync -av LICENSE-APACHE $distdir/
rsync -av LICENSE-MIT $distdir/
rsync -av README.md $distdir/
cd $workdir
tar czvf ${cwd}/ccsds_${version}.tar.gz $target
