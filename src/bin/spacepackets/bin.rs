mod info;
mod merge;

use std::process;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Inspect the contents of a file
    Info {
        /// Input spacepacket file
        #[arg(short, long)]
        input: String,

        /// Show gap information
        #[arg(short, long)]
        gaps: bool,

        /// Timecode format
        #[arg(value_enum, short, long, default_value_t = TimecodeFormat::None)]
        timecode: TimecodeFormat,
    },
    /// Merge multiple files together
    Merge {
        #[arg(short, long)]
        input: String,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum TimecodeFormat {
    /// CCSDS Day Segmented timecode
    CDS,
    /// CCSDS Unsegmented timecode as used in NASA EOS Mission
    EOSCUC,
    /// Do not decode times
    None,
}

fn main() -> process::ExitCode {
    let cli = Cli::parse();
    println!("{:?}", cli);

    match &cli.command {
        Commands::Info {
            input,
            gaps,
            timecode,
        } => info::do_info(input.clone(), *gaps, timecode),
        Commands::Merge { input } => merge::do_merge(input.clone()),
    }
}
