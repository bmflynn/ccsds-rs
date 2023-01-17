mod info;
mod merge;
mod inspect;

use std::process;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about=None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum TimecodeFormat {
    CDS,
    EOSCUC,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Inspect the contents of a file
    Info {
        /// Input spacepacket file
        #[arg(short, long)]
        input: String,

        /// Timecode format
        #[arg(value_enum, short, long, default_value_t=TimecodeFormat::CDS)]
        timecode: TimecodeFormat,
    },
    /*
    /// Merge multiple files together
    Merge {
        #[arg(short, long)]
        input: String,
    },
    */
}


fn main() -> process::ExitCode {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Info {
            input,
            timecode,
        } => info::do_info(input.clone(), timecode),
        // Commands::Merge { input } => merge::do_merge(input.clone()),
    }
}
