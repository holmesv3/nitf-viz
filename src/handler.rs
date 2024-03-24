//! Definition of image reading/writing logic
use core::slice;
use std::fs::File;
use rayon::prelude::*;
use thiserror::Error;
use image::{
    codecs::gif::{GifEncoder, Repeat},
    imageops::{thumbnail, colorops::{brighten_in_place, contrast_in_place}},
    DynamicImage, Frame, ImageBuffer, Luma, Rgb,
};
use log::{error, debug, info, trace};
use memmap2::Mmap;
use nitf_rs::headers::image_hdr::*;
use nitf_rs::Nitf;

use crate::cli::Cli;


#[derive(Error, Debug)]
pub enum VizError {
    #[error("Nitf ImageRepresentation::{0} is not implemented")]
    Irep(ImageRepresentation),
    #[error("Non 8-bit is not implemented for this ImageRepresentation")]
    Nbpp,
    #[error(transparent)]
    ImageError(#[from] image::error::ImageError),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    NitfError(#[from] nitf_rs::NitfError),
    
}
type VizResult<T> = Result<T, VizError>;

pub struct ImageWrapper {
    /// Number of Significant Rows in image
    pub nrows: u32,
    /// Number of Significant Columns in image
    pub ncols: u32,
    /// Pixel Value Type
    pub pvtype: PixelValueType,
    /// Image Representation
    pub irep: ImageRepresentation,
    /// Image Compression
    pub ic: Compression,
    /// Number of Bands
    pub nbands: u8,
    /// Number of Bits Per Pixel
    pub nbpp: u8,
    /// Actual Bits-Per-Pixel Per Band
    pub abpp: u8,
    /// Number of Blocks per Column
    pub nbpc: u16,
    /// Number of Blocks per Row
    pub nbpr: u16,
    /// Image Mode
    pub imode: Mode,
    /// Data bands
    pub bands: Vec<Band>,
    /// Number of Pixels Per Block Horizontal
    pub nppbh: u16,
    /// Number of Pixels Per Block Vertical
    pub nppbv: u16,
    /// Output brightness adjustment
    pub brightness: i32,
    /// Output contrast adjustment
    pub contrast: f32,
    data: Mmap,
}

impl ImageWrapper {
    
    fn read_image(&self) -> VizResult<DynamicImage> {
        match self.irep {
            ImageRepresentation::MONO => self.read_mono(),
            ImageRepresentation::RGB => self.read_rgb(),     
            ImageRepresentation::RGBLUT => self.read_rgb_lut(),
            ImageRepresentation::NODISPLY => self.read_nodisplay(),
            unimpl => Err(VizError::Irep(unimpl)),
        }
    }
    
    
    pub fn get_image(&self, size: u32) -> VizResult<DynamicImage> {
        trace!("Found nrows: {}", self.nrows);
        trace!("Found ncols: {}", self.ncols);
        trace!("Found pvtype: {}", self.pvtype);
        trace!("Found irep: {}", self.irep);
        trace!("Found ic: {}", self.ic);
        trace!("Found nbands: {}", self.nbands);
        trace!("Found nbpp: {}", self.nbpp);
        trace!("Found abpp: {}", self.abpp);
        trace!("Found nbpc: {}", self.nbpc);
        trace!("Found nbpr: {}", self.nbpr);
        trace!("Found imode: {}", self.imode);
        trace!("Found nppbh: {}", self.nppbh);
        trace!("Found nppbv: {}", self.nppbv);
        self.bands.iter().enumerate().for_each(|(i_b, band)| {
            trace!("Band {i_b}");
            trace!("Found irepband: {}", band.irepband.val);
            trace!("Found isubcat: {}", band.isubcat.val);
            trace!("Found ifc: {}", band.ifc.val);
            trace!("Found imflt: {}", band.imflt.val);
            trace!("Found nluts: {}", band.nluts.val);
            trace!("Found nelut: {}", band.nelut.val);
        });
        trace!("Data length = {}", &self.data.len());

        let image = match self.read_image() {
            Ok(im) => im,
            Err(e) => {error!("{e}"); return Err(e);},
        };
                
        // Make thumbnail
        let aspect = self.ncols as f32 / self.nrows as f32;
        debug!("Original dimensions: {} X {}", self.ncols, self.nrows);

        let max_size = size.pow(2) as f32;
        let new_width = (aspect * max_size).sqrt() as u32;
        let new_height = (max_size / new_width as f32) as u32;
        debug!("Thumbnail dimensions: {new_width} X {new_height}");
        Ok(thumbnail(&image, new_width, new_height).into())
    }
    
    /// Read an mono represented image. Currently assumes all data is a single byte
    fn read_mono(&self) -> VizResult<DynamicImage> {
        if self.nbpp != 8 {
            return Err(VizError::Nbpp)
        }
        let mut image = ImageBuffer::new(self.ncols, self.nrows);
        self.data
            .par_iter()
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| {
                *px = Luma([*data]);
            });
        brighten_in_place(&mut image, self.brightness);
        contrast_in_place(&mut image, self.contrast);
        Ok(image.into())
    }

    /// Read an rgb represented image. Currently assumes all data is a single byte
    fn read_rgb(&self) -> VizResult<DynamicImage> {
        if self.nbpp != 8 {
            return Err(VizError::Nbpp)
        }
        let mut image = ImageBuffer::new(self.ncols, self.nrows);
        self.data
            .par_chunks(3)
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| {
                *px = Rgb([data[0], data[1], data[2]]);
            });
        brighten_in_place(&mut image, self.brightness);
        contrast_in_place(&mut image, self.contrast);
        Ok(image.into())
    }

    /// Read an rgb_lut represented image. Currently assumes all data is a single byte
    fn read_rgb_lut(&self) -> VizResult<DynamicImage> {
        if self.nbpp != 8 {
            return Err(VizError::Nbpp)
        }
        let (r, g, b) = (0, 1, 2);
        let lut = &self.bands[0].lutd;
        let mut image = ImageBuffer::new(self.ncols, self.nrows);
        self.data
            .par_iter()
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| {
                let idx = *data as usize;
                *px = Rgb([lut[r][idx], lut[g][idx], lut[b][idx]]);
            });
        brighten_in_place(&mut image, self.brightness);
        contrast_in_place(&mut image, self.contrast);
        Ok(image.into())
    }
    
    /// For now, assume NODISPLY imagery is SICD (Complex Float) and plot magnitude
    fn read_nodisplay(&self) -> VizResult<DynamicImage> {
        let recast = self.data.as_ptr() as *const [[u8; 4]; 2];
        let slice_len = self.data.len() / 8;
        let new_slice = unsafe {slice::from_raw_parts(recast, slice_len)};
        
        let mut image = ImageBuffer::new(self.ncols, self.nrows);
        trace!("Recast to from data length {} bytes to {} X {} bytes", self.data.len(), slice_len, 64);
        debug!("Start complex pixel remap");
        // Remap initially into a f32 array of the magnitude, then remap to power
        (0..slice_len).into_par_iter()
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| {
                let complex = &new_slice[data];
                let real = f32::from_be_bytes(complex[0]);
                let imag = f32::from_be_bytes(complex[1]);
                let val = 20_f32 * (real.powi(2) + imag.powi(2)).sqrt().log10();
                *px = Luma([val]);
            }
        );
        brighten_in_place(&mut image, self.brightness);
        contrast_in_place(&mut image, self.contrast);
        debug!("Done remapping");
        Ok(image.into())
    }
}

#[derive(Debug, Clone)]
pub struct ImageConfig {}

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
}

/// Takes care of all reading, parsing, and writing work
impl Handler {
    fn get_image(&self, i_seg: usize) -> VizResult<DynamicImage> {
        self.wrappers[i_seg].get_image(self.size)
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
            let frame = Frame::new(image.into());
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
                    brightness: args.brightness,
                    contrast: args.contrast,

                    data: seg.get_data_map(&mut nitf_file).unwrap(),
                }
            })
            .collect();

        Ok(Self {
            numi,
            stem,
            out_dir,
            size,
            wrappers,
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
        // num1 > 1
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
