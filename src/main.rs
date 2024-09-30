//! Attempt to read and write thumbnail/gif of image data from a nitf
use clap::Parser;
use log::{info, LevelFilter};
use memmap2::Mmap;
use ndarray::ArrayView4;
use nitf_rs::headers::image_hdr::ImageRepresentation;
use simple_logger::SimpleLogger;
use std::ops::Index;
use thiserror::Error;

mod cli;
mod handler;
mod mono;
mod rgb;
mod rgb_lut;
mod sicd;

use cli::Cli;
use handler::Handler;

pub type VizResult<T> = Result<T, VizError>;

#[derive(Error, Debug)]
pub enum VizError {
    #[error("Reached non-implemented path")]
    Unimplemented,
    #[error("Something is bad")]
    DoBetter,
    #[error("Nitf ImageRepresentation::{0} is not implemented")]
    Irep(ImageRepresentation),
    #[error("Non 8-bit-aligned data is not implemented")]
    Nbpp,
    #[error(transparent)]
    ImageError(#[from] image::error::ImageError),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    XmlError(#[from] quick_xml::Error),
    #[error(transparent)]
    NitfError(#[from] nitf_rs::NitfError),
}

// This assumes P image mode (band data for each pixel is sequential)
const ARRAY_ROW_IDX: usize = 2;
const ARRAY_COL_IDX: usize = 3;

struct BlockedPaddedArray<T: 'static> {
    pub array: ArrayView4<'static, T>,
    pub n_rows: usize,
    pub n_cols: usize,
    _mmap: Mmap,
}

impl<T: 'static> BlockedPaddedArray<T> {
    pub fn unwrap_idx(&self, index: [usize; 2]) -> Option<&T> {
        let n_elem_row = self.array.shape()[ARRAY_ROW_IDX];
        let n_elem_col = self.array.shape()[ARRAY_COL_IDX];
        let in_row = index[0];
        let in_col = index[1];
        // If the index would fall outside of the array, return None
        if in_row >= self.n_rows {
            return None;
        }
        // This assumes that data is split along rows for each segment
        if in_col >= self.n_cols {
            panic!("Index out of bounds")
        }

        let row_block_idx = in_row / n_elem_row;
        let col_block_idx = in_col / n_elem_col;

        let row_idx = in_row % n_elem_row;
        let col_idx = in_col % n_elem_col;
        let ret = [row_block_idx, col_block_idx, row_idx, col_idx];
        // println!("{:?}", &index);
        // println!("{:?}", &ret);
        Some(&self.array[ret])
    }
}

struct Stack<T: 'static> {
    pub arrs: Vec<BlockedPaddedArray<T>>,
}

impl<T: 'static> Index<[usize; 2]> for Stack<T> {
    type Output = T;
    fn index(&self, index: [usize; 2]) -> &Self::Output {
        let mut in_row = index[0];
        let in_col = index[1];
        for arr in &self.arrs {
            let val = arr.unwrap_idx([in_row, in_col]);
            if let Some(idx) = val {
                return idx;
            }
            // Assumes segments split the data across its columns
            in_row -= arr.n_rows;
        }
        panic!("Index {index:?} not valid");
    }
}

fn main() {
    let args = Cli::parse();
    // Configure logging
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

    // Run on each input file
    for input in &args.input {
        let out_file = Handler::run(input, &args).unwrap();
        info!("Finished writing {out_file}");
    }
}
