mod edit;
mod inspect;
mod merge;

use std::process;

use ccsds::spacepacket::APID;
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
    Edit {
        #[arg(short, long)]
        input: String,
        #[arg(short, long, action=clap::ArgAction::Append)]
        apids: Vec<APID>,
        #[arg(short, long)]
        start: Option<String>,
        #[arg(short, long)]
        end: Option<String>,
        #[arg(value_enum, short, long, default_value_t=TimecodeFormat::CDS)]
        tc_fmt: TimecodeFormat,
    },
    /// Merge multiple files together
    Merge {
        #[arg(short, long)]
        input: String,
    },
}

fn main() -> process::ExitCode {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Edit {
            input,
            apids,
            start,
            end,
            tc_fmt,
        } => edit::do_edit(input.clone(), apids.clone(), start.clone(), end.clone(), tc_fmt),
        Commands::Merge { input } => merge::do_merge(input.clone()),
    }
}
