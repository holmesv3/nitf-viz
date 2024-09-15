//! Definition of image reading/writing logic
use image::{
    codecs::gif::{GifEncoder, Repeat},
    imageops::colorops::{brighten_in_place, contrast_in_place},
    Frame, RgbaImage,
};
use log::{debug, info};
use nitf_rs::Nitf;
use std::fs::File;

use crate::cli::Cli;
use crate::image_wrapper::ImageWrapper;
use crate::sicd::run as run_sicd;
use crate::{VizError, VizResult};

// #[derive(Debug, Clone)]
// pub struct ImageConfig {}

/// Top level handler for all program logic
pub struct Handler {
    /// Number of image segments
    pub numi: u16,
    /// Inpuf file name
    pub stem: String,
    /// Input file path
    pub input: std::path::PathBuf,
    /// Output directory
    pub out_dir: std::path::PathBuf,
    /// Output image(s) size
    pub size: u32,
    /// Images
    pub wrappers: Vec<ImageWrapper>,
}

fn init_handler(input: &std::path::PathBuf, args: &Cli) -> VizResult<Handler> {
    let stem = input
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap()
                .to_string();
        

        let size = args.size;
        let out_dir = args.output.clone();

        let _ = match out_dir
            .try_exists()
            .expect("Don't have permission for that folder")
        {
            false => std::fs::create_dir_all(&out_dir),
            true => Ok(()),
        };

        debug!("Reading {:}", input.to_str().unwrap());
        let mut nitf_file = File::open(input.clone())?;
        let nitf = Nitf::from_reader(&mut nitf_file)?;
        let numi = nitf.nitf_header.numi.val;
        debug!("Found numi = {numi}");

        let wrappers = nitf
            .image_segments
            .iter()
            .map(|seg| {
                let meta = &seg.header;
                let data = seg.get_data_map(&mut nitf_file).unwrap();
                ImageWrapper {
                    nrows: meta.nrows.val,
                    ncols: meta.ncols.val,
                    pvtype: meta.pvtype.val,
                    ic: meta.ic.val,
                    nbpp: meta.nbpp.val,
                    abpp: meta.abpp.val,
                    nbands: meta.nbands.val,
                    irep: meta.irep.val,
                    nbpc: meta.nbpc.val,
                    nbpr: meta.nbpr.val,
                    imode: meta.imode.val,
                    nppbh: meta.nppbh.val,
                    nppbv: meta.nppbv.val,
                    bands: meta.bands.clone(),
                    data,
                }
            })
            .collect();

        Ok(Handler {
            numi,
            stem,
            out_dir,
            wrappers,
            size,
            input: input.clone(),
        })
}

/// Takes care of all reading, parsing, and writing work
impl Handler {
    fn init() {
        
    }
    fn get_image(&self, i_seg: usize) -> VizResult<RgbaImage> {
        self.wrappers[i_seg].get_image(self.size)
    }
    pub fn single_segment(&self, i_seg: usize, stem: &str) -> VizResult<()> {
        let out_file = self.out_dir.join(format!("{stem}.png"));
        let image = self.get_image(i_seg)?;
        image.save(&out_file)?;
        info!("Finished writing {}", out_file.to_str().unwrap());
        Ok(())
    }

    pub fn multi_segment(&self, stem: &str) -> VizResult<()> {
        let out_file = self.out_dir.join(format!("{stem}.gif"));
        let gif_file = File::create(&out_file)?;

        let mut encoder = GifEncoder::new_with_speed(gif_file, 1);
        let _ = encoder.set_repeat(Repeat::Infinite);
        for i_seg in 0..self.numi {
            let image = self.get_image(i_seg.into())?;
            info!("Writing frame {} of {}", i_seg + 1, self.numi);
            let frame = Frame::new(image);
            let _ = encoder.encode_frame(frame);
        }
        info!("Finished writing {}", out_file.to_str().unwrap());
        Ok(())
    }
}

pub fn run(args: &Cli) -> VizResult<()> {
    for input in &args.input
    {
        let obj: Handler = init_handler(input, args)?;
        let stem = &obj.stem;
    
        let is_sicd = sicd_rs::read_sicd(&input).is_ok();
        if is_sicd {
            run_sicd(obj)?;
        }
        // Only dealing with a single image.
        else if obj.numi == 1 {
            obj.single_segment(0, stem)?;
        } else {
            // numi > 1
            obj.multi_segment(stem)?;
        }
    }
    Ok(())

}
