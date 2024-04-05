//! Attempt to read and write thumbnail/gif of image data from a nitf
use clap::Parser;
use log::LevelFilter;
use nitf_rs::headers::image_hdr::ImageRepresentation;
use simple_logger::SimpleLogger;
use thiserror::Error;
mod cli;
mod handler;
mod remap;

use cli::Cli;
use handler::run;

pub(crate) type C32Layout = [[u8; 4]; 2];
pub type VizResult<T> = Result<T, VizError>;

#[derive(Error, Debug)]
pub enum VizError {
    #[error("Nitf ImageRepresentation::{0} is not implemented")]
    Irep(ImageRepresentation),
    #[error("Non 8-bit-aligned data is not implemented")]
    Nbpp,
    #[error(transparent)]
    ImageError(#[from] image::error::ImageError),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    NitfError(#[from] nitf_rs::NitfError),
}

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
