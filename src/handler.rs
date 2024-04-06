//! Definition of image reading/writing logic
use core::slice;
use image::{
    codecs::gif::{GifEncoder, Repeat},
    imageops::{
        colorops::{brighten_in_place, contrast_in_place},
        thumbnail,
    },
    Frame, Rgba, RgbaImage,
};
use log::{debug, error, info, trace};
use memmap2::Mmap;
use nitf_rs::headers::image_hdr::*;
use nitf_rs::Nitf;
use rayon::prelude::*;
use std::fs::File;

use crate::cli::Cli;
use crate::remap::Pedf;
use crate::{C32Layout, VizError, VizResult};

// TODO:
//      Need to implement some kind of 'manual' convolutional down-sampling,
//      in the case of very large data
//      Construction of a single image from multiple segments

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
    data: Mmap,
    remap: Option<Pedf>,
}

#[derive(Default, Debug, Clone, Copy)]
struct BlockInfo {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl ImageWrapper {
    fn read_image(&self) -> VizResult<RgbaImage> {
        if self.nbpp % 8 != 0 {
            return Err(VizError::Nbpp);
        }

        let block_height = self.nppbv as u32;
        let block_width = self.nppbh as u32;
        let block_per_row = self.nbpr as u32;
        let block_per_col = self.nbpc as u32;

        let ncols = {
            if block_per_row < 1 {
                block_per_row * block_width
            } else {
                self.ncols
            }
        };
        let nrows = {
            if block_per_col < 1 {
                block_per_col * block_height
            } else {
                self.nrows
            }
        };

        let mut image = RgbaImage::new(ncols, nrows);

        // If the image is not 'blocked',
        if self.nbpr == 1 && self.nbpc == 1 {
            match self.irep {
                ImageRepresentation::MONO => self.read_mono(&mut image),
                ImageRepresentation::RGB => self.read_rgb(&mut image),
                ImageRepresentation::RGBLUT => self.read_rgb_lut(&mut image),
                ImageRepresentation::NODISPLY => self.read_nodisplay(&mut image),
                unimpl => Err(VizError::Irep(unimpl)),
            }?;
            return Ok(image);
        }
        let byte_per_px = (self.nbpp / 8) as u32;

        let n_block = block_per_row * block_per_col;
        let mut block_info = vec![BlockInfo::default(); n_block as usize];
        for i_y in 0..block_per_col {
            let y = i_y * block_height;
            for i_x in 0..block_per_row {
                let x = i_x * block_width;
                let block_idx = i_x as usize + (i_y * block_per_row) as usize;
                block_info[block_idx] = BlockInfo {
                    x,
                    y,
                    width: block_width,
                    height: block_height,
                };
            }
        }
        let chunk_size = (byte_per_px * block_width * block_height) as usize;
        let data_chunks = self.data.chunks_exact(chunk_size);
        block_info
            .iter()
            .zip(data_chunks)
            .try_for_each(|(block, chunk)| {
                match self.irep {
                    ImageRepresentation::MONO => self.blocked_read_mono(chunk, block, &mut image),
                    unimpl => Err(VizError::Irep(unimpl)),
                }
            })?;

        Ok(image)
    }

    pub fn get_image(&self, size: u32) -> VizResult<RgbaImage> {
        trace!("IMAGE INFO");
        trace!("| Found nrows: {}", self.nrows);
        trace!("| Found ncols: {}", self.ncols);
        trace!("| Found pvtype: {}", self.pvtype);
        trace!("| Found irep: {}", self.irep);
        trace!("| Found ic: {}", self.ic);
        trace!("| Found nbands: {}", self.nbands);
        trace!("| Found nbpp: {}", self.nbpp);
        trace!("| Found abpp: {}", self.abpp);
        trace!("| Found imode: {}", self.imode);

        trace!("BLOCK INFO");
        trace!("| Found nbpc: {}", self.nbpc);
        trace!("| Found nbpr: {}", self.nbpr);
        trace!("| Found nppbh: {}", self.nppbh);
        trace!("| Found nppbv: {}", self.nppbv);

        trace!("BAND INFO");
        self.bands.iter().enumerate().for_each(|(i_b, band)| {
            trace!("| Band {i_b}");
            trace!("| | Found irepband: {}", band.irepband.val);
            trace!("| | Found isubcat: {}", band.isubcat.val);
            trace!("| | Found ifc: {}", band.ifc.val);
            trace!("| | Found imflt: {}", band.imflt.val);
            trace!("| | Found nluts: {}", band.nluts.val);
            trace!("| | Found nelut: {}", band.nelut.val);
        });
        trace!("Data length = {}", &self.data.len());

        let image = match self.read_image() {
            Ok(im) => im,
            Err(e) => {
                error!("{e}");
                return Err(e);
            }
        };

        // Make thumbnail
        let aspect = self.ncols as f32 / self.nrows as f32;
        debug!("Original dimensions: {} X {}", self.ncols, self.nrows);

        let max_size = size.pow(2) as f32;
        let new_width = (aspect * max_size).sqrt() as u32;
        let new_height = (max_size / new_width as f32) as u32;
        debug!("Thumbnail dimensions: {new_width} X {new_height}");
        Ok(thumbnail(&image, new_width, new_height))
    }

    /// Read an mono represented image. Currently assumes all data is a single byte
    fn read_mono(&self, image: &mut RgbaImage) -> VizResult<()> {
        if self.nbpp != 8 {
            return Err(VizError::Nbpp);
        };

        self.data
            .par_iter()
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| *px = Rgba([*data, *data, *data, u8::MAX]));

        Ok(())
    }

    /// Read an mono represented image. Currently assumes all data is a single byte
    fn blocked_read_mono(
        &self,
        data: &[u8],
        block: &BlockInfo,
        image: &mut RgbaImage,
    ) -> VizResult<()> {
        // Make values outside of "significant" image data transparent
        let alpha = |x: u32, y: u32| {
            if x >= self.ncols || y >= self.nrows {
                u8::MIN
            } else {
                u8::MAX
            }
        };

        if self.nbpp != 8 {
            return Err(VizError::Nbpp);
        };

        let mut block_iter = vec![(0_u32, 0_u32); (block.width * block.height) as usize];
        for (i_y, y) in (block.y..(block.y + block.height)).enumerate() {
            for (i_x, x) in (block.x..(block.x + block.width)).enumerate() {
                block_iter[i_x + i_y * block.width as usize] = (x, y)
            }
        }

        let block_iter = block_iter.iter().cloned();
        for (data, (x, y)) in data.iter().zip(block_iter) {
            image.put_pixel(x, y, Rgba([*data, *data, *data, alpha(x, y)]));
        }

        Ok(())
    }

    /// Read an rgb represented image. Currently assumes all data is a single byte
    fn read_rgb(&self, image: &mut RgbaImage) -> VizResult<()> {
        if self.nbpp != 8 {
            return Err(VizError::Nbpp);
        }

        self.data
            .par_chunks(3)
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| *px = Rgba([data[0], data[1], data[2], u8::MAX]));

        Ok(())
    }

    /// Read an rgb_lut represented image. Currently assumes all data is a single byte
    fn read_rgb_lut(&self, image: &mut RgbaImage) -> VizResult<()> {
        if self.nbpp != 8 {
            return Err(VizError::Nbpp);
        }
        let (r, g, b) = (0, 1, 2);
        let lut = &self.bands[0].lutd;
        self.data
            .par_iter()
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| {
                let idx = *data as usize;
                *px = Rgba([lut[r][idx], lut[g][idx], lut[b][idx], u8::MAX]);
            });
        Ok(())
    }

    /// For now, assume NODISPLY imagery is SICD (Complex32) and plot magnitude
    fn read_nodisplay(&self, image: &mut RgbaImage) -> VizResult<()> {
        let data = &self.data;
        let recast = data.as_ptr() as *const C32Layout;
        let slice_len = data.len() / 8;
        let new_slice = unsafe { slice::from_raw_parts(recast, slice_len) };
        trace!(
            "Recast to from data length {} bytes to {} X {} bytes",
            data.len(),
            slice_len,
            64
        );
        debug!("Start complex pixel remap");
        new_slice
            .into_par_iter()
            .zip(image.par_pixels_mut())
            .for_each(|(data, px)| {
                let val = self.remap.unwrap().remap(data);
                *px = Rgba([val, val, val, u8::MAX]);
            });

        debug!("Done remapping");
        Ok(())
    }
}

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
