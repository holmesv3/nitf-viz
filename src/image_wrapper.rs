//! Definition of image reading/writing logic
use image::{imageops::thumbnail, Rgba, RgbaImage};
use log::{debug, error, trace};
use memmap2::Mmap;
use nitf_rs::headers::image_hdr::*;
use rayon::prelude::*;

use crate::{VizError, VizResult};

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
    /// Data on disk
    pub data: Mmap,
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
            if block_per_row < 2 {
                self.ncols
            } else {
                block_per_row * block_width
            }
        };
        let nrows = {
            if block_per_col < 2 {
                self.nrows
            } else {
                block_per_col * block_height
            }
        };

        let mut image = RgbaImage::new(ncols, nrows);

        // If the image is not 'blocked',
        if self.nbpr == 1 && self.nbpc == 1 {
            match self.irep {
                ImageRepresentation::MONO => self.read_mono(&mut image),
                ImageRepresentation::RGB => self.read_rgb(&mut image),
                ImageRepresentation::RGBLUT => self.read_rgb_lut(&mut image),
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
        let chunk_size = (byte_per_px * block_width * block_height * self.nbands as u32) as usize;
        let data_chunks = self.data.chunks_exact(chunk_size);
        block_info
            .iter()
            .zip(data_chunks)
            .try_for_each(|(block, chunk)| match self.irep {
                ImageRepresentation::MONO => self.blocked_read_mono(chunk, block, &mut image),
                ImageRepresentation::RGB => self.blocked_read_rgb(chunk, block, &mut image),
                ImageRepresentation::RGBLUT => self.blocked_read_rgblut(chunk, block, &mut image),
                unimpl => Err(VizError::Irep(unimpl)),
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
        debug!("Original dimensions: {} X {}", self.nrows, self.ncols);

        let max_size = size.pow(2) as f32;
        let new_width = (aspect * max_size).sqrt() as u32;
        let new_height = (max_size / new_width as f32) as u32;
        debug!("Thumbnail dimensions: {new_height} X {new_width}");
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

    /// Read an rgb represented image
    fn blocked_read_rgb(
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

        let (r, g, b) = (0, 1, 2);
        let block_iter = block_iter.iter().cloned();
        for (data, (x, y)) in data.chunks_exact(3).zip(block_iter) {
            image.put_pixel(
                x,
                y,
                Rgba([data[r], data[g], data[b], alpha(x, y)]),
            );
        }

        Ok(())
    }
    
    /// Read an rgblut represented image. Currently assumes all data is a single byte
    fn blocked_read_rgblut(
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

        let (r, g, b) = (0, 1, 2);
        let lut = &self.bands[0].lutd;
        let block_iter = block_iter.iter().cloned();
        for (data, (x, y)) in data.iter().zip(block_iter) {
            let idx = *data as usize;
            image.put_pixel(
                x,
                y,
                Rgba([lut[r][idx], lut[g][idx], lut[b][idx], alpha(x, y)]),
            );
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
}
