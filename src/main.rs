//! Attempt to read and write thumbnail/gif of image data from a nitf
use clap::Parser;
use log::LevelFilter;
use simple_logger::SimpleLogger;

mod cli;
mod handler;

use cli::Cli;
use handler::run;

fn main() {
    let args = Cli::parse();
    let nitf_rs_lvl = if !args.nitf_log {
        LevelFilter::Off
    } else {
        args.level.into()
    };
    SimpleLogger::new()
        .with_level(args.level.into())
        .with_module_level("nitf_rs", nitf_rs_lvl)
        .init()
        .unwrap();

    // This function wraps all program logic
    run(&args).unwrap();
}
