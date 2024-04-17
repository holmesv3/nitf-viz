//! Definition of image reading/writing logic
use image::{
    codecs::gif::{GifEncoder, Repeat},
    imageops::colorops::{brighten_in_place, contrast_in_place},
    Frame, RgbaImage,
};
use log::{debug, info};
use nitf_rs::headers::image_hdr::*;
use nitf_rs::Nitf;
use std::fs::File;

use crate::cli::Cli;
use crate::image_wrapper::ImageWrapper;
use crate::remap::Pedf;
use crate::{VizError, VizResult};

// #[derive(Debug, Clone)]
// pub struct ImageConfig {}

/// Top level handler for all program logic
pub struct Handler {
    /// Number of image segments
    pub numi: u16,
    /// Inpuf file name
    pub stem: String,
    /// Output directory
    pub out_dir: std::path::PathBuf,
    /// Output image(s) size
    pub size: u32,
    /// Images
    pub wrappers: Vec<ImageWrapper>,
    /// Output brightness adjustment
    pub brightness: i32,
    /// Output contrast adjustment
    pub contrast: f32,
}

/// Takes care of all reading, parsing, and writing work
impl Handler {
    fn get_image(&self, i_seg: usize) -> VizResult<RgbaImage> {
        let mut image = self.wrappers[i_seg].get_image(self.size)?;
        if self.brightness != 0 {
            debug!("Adjusting brightness");
            brighten_in_place(&mut image, self.brightness);
        }
        if self.contrast != 0.0 {
            debug!("Adjusting contrast");
            contrast_in_place(&mut image, self.contrast);
        }
        Ok(image)
    }
    pub fn single_segment(&self, i_seg: usize, stem: &str) -> VizResult<()> {
        let out_file = self.out_dir.join(format!("{}_{}.png", stem, self.size));
        let image = self.get_image(i_seg)?;
        image.save(&out_file)?;
        info!("Finished writing {}", out_file.to_str().unwrap());
        Ok(())
    }

    pub fn multi_segment(&self, stem: &str) -> VizResult<()> {
        let out_file = self.out_dir.join(format!("{}_{}.gif", stem, self.size));
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

impl TryFrom<&Cli> for Handler {
    type Error = VizError;
    fn try_from(args: &Cli) -> VizResult<Self> {
        let stem = args
            .input
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

        debug!("Reading {:}", args.input.to_str().unwrap());
        let mut nitf_file = File::open(args.input.clone())?;
        let nitf = Nitf::from_reader(&mut nitf_file)?;
        let numi = nitf.nitf_header.numi.val;
        debug!("Found numi = {numi}");

        let wrappers = nitf
            .image_segments
            .iter()
            .map(|seg| {
                let meta = &seg.header;
                let data = seg.get_data_map(&mut nitf_file).unwrap();

                let remap = match meta.irep.val {
                    ImageRepresentation::NODISPLY => Some(Pedf::init(
                        &data,
                        meta.nrows.val as usize,
                        meta.ncols.val as usize,
                    )),
                    _ => None,
                };
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
                    remap,
                    data,
                }
            })
            .collect();

        Ok(Self {
            numi,
            stem,
            out_dir,
            wrappers,
            size,
            brightness: args.brightness,
            contrast: args.contrast,
        })
    }
}

pub fn run(args: &Cli) -> VizResult<()> {
    let obj: Handler = args.try_into()?;
    let stem = &obj.stem;
    // Only dealing with a single image.
    if obj.numi == 1 {
        obj.single_segment(0, stem)?;
    } else {
        // numi > 1
        if args.individual {
            for i_seg in 0..obj.numi {
                obj.single_segment(i_seg.into(), &format!("{stem}_{i_seg}"))?;
            }
        } else {
            obj.multi_segment(stem)?;
        }
    }
    Ok(())
}
