use anyhow::Result;
use ccsds::framing::Derandomizer;
use std::{fs::File, io::Write, path::PathBuf};

pub fn synchronize(
    input: PathBuf,
    block_len: usize,
    pn: bool,
    no_asm: bool,
    output: Option<PathBuf>,
    verbose: bool,
) -> Result<()> {
    let reader = File::open(input)?;
    let mut output = match output {
        Some(fpath) => Some(File::create(fpath)?),
        None => None,
    };
    let asm = &ccsds::framing::ASM[..];
    let opts = ccsds::framing::SyncOpts::new(block_len).with_asm(&asm);
    let denoiser = ccsds::framing::DefaultDerandomizer::default();

    for each in ccsds::framing::synchronize(reader, opts) {
        match each {
            Ok(mut block) => {
                if let Some(f) = output.as_mut() {
                    if !no_asm {
                        f.write_all(&asm)?;
                    }
                    if pn {
                        f.write_all(&denoiser.derandomize(&mut block.data))?;
                    } else {
                        f.write_all(&block.data)?;
                    }
                }
                let loc = block.loc;
                if verbose {
                    println!("byte_offset={} bit_offset={}", loc.offset, loc.bit);
                }
            }
            Err(e) => eprintln!("{e}"),
        }
    }

    Ok(())
}
