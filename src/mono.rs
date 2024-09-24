//! Definition of image reading/writing logic
use std::fs::File;

use image::{GrayImage, Luma};

use log::debug;
use ndarray::parallel::prelude::*;
use ndarray::ArrayView4;
use nitf_rs::headers::image_hdr::Mode;
use nitf_rs::Nitf;

use crate::handler::Handler;
use crate::VizResult;
use crate::{BlockedPaddedArray, Stack};

pub fn make_mono(handler: Handler, stem: String) -> VizResult<String> {
    let size = handler.size;
    let out_dir = &handler.out_dir;

    let _ = match out_dir
        .try_exists()
        .expect("Don't have permission for that folder")
    {
        false => std::fs::create_dir_all(&out_dir),
        true => Ok(()),
    };

    debug!("Reading {:}", handler.input.to_str().unwrap());
    let mut nitf_file = File::open(handler.input.clone())?;
    let nitf = Nitf::from_reader(&mut nitf_file)?;

    // Map out the full image from the individual segments
    let arrs: Vec<BlockedPaddedArray<u8>> = nitf
        .image_segments
        .iter()
        .map(|s| {
            // n_rows and n_cols are the significant pixels, there may be fill data
            let n_rows = s.header.nrows.val as usize;
            let n_cols = s.header.ncols.val as usize;

            //
            let block_height = s.header.nppbv.val as usize;
            let block_width = s.header.nppbh.val as usize;
            let block_per_row = s.header.nbpr.val as usize;
            let block_per_col = s.header.nbpc.val as usize;

            let ptr = s.get_data_map(&mut nitf_file).unwrap().as_ptr();
            let array = unsafe {
                ArrayView4::from_shape_ptr(
                    (block_height, block_width, block_per_row, block_per_col),
                    ptr,
                )
            };
            BlockedPaddedArray {
                array,
                n_rows,
                n_cols,
            }
        })
        .collect();

    // This assumes all segments have the same number of columns, and are split along rows
    let n_rows = arrs.iter().fold(0, |acc, arr| acc + arr.n_rows as u32);
    let n_cols = arrs[0].n_cols as u32;

    let stack = Stack { arrs };

    debug!("Creating image");
    // Determine the input aspect ratio and chunk size
    let aspect = (n_cols as f64) / (n_rows as f64);

    let max_size = size.pow(2) as f64;
    let out_cols = (aspect * max_size).sqrt() as u32;
    let out_rows = (max_size / out_cols as f64) as u32;

    let x_ratio = n_cols as f32 / out_cols as f32;
    let y_ratio = n_rows as f32 / out_rows as f32;

    let mut image = GrayImage::new(out_cols, out_rows);

    // TODO: Need to abstract this somehow
    // Zip::indexed(&mut image.p).par_for_each(|(outy, outx), elem| {
    image
        .par_enumerate_pixels_mut()
        .for_each(|(outx, outy, elem)| {
            let bottomf = outy as f32 * y_ratio;
            let topf = bottomf + y_ratio;

            let bottom = (bottomf.ceil() as u32).clamp(0, n_rows - 1);
            let top = (topf.ceil() as u32).clamp(bottom, n_rows);
            let leftf = outx as f32 * x_ratio;
            let rightf = leftf + x_ratio;

            let left = (leftf.ceil() as u32).clamp(0, n_cols - 1);
            let right = (rightf.ceil() as u32).clamp(left, n_cols);

            if bottom != top && left != right {
                let n = ((top - bottom) * (right - left)) as f32;
                let mut res = 0_f32;
                for i_row in bottom as usize..top as usize {
                    for i_col in left as usize..right as usize {
                        res += stack[[i_row, i_col]] as f32
                    }
                }
                *elem = Luma([(res / n) as u8]);
            } else if bottom != top {
                let fract = (leftf.fract() + rightf.fract()) / 2.;

                let mut sum_left = 0_u32;
                let mut sum_right = 0_u32;
                for x in bottom as usize..top as usize {
                    sum_left += stack[[x, left as usize]] as u32;
                    sum_right += stack[[x, left as usize + 1]] as u32;
                }

                // Now we approximate: left/n*(1-fract) + right/n*fract
                let fact_right = fract / ((top - bottom) as f32);
                let fact_left = (1. - fract) / ((top - bottom) as f32);

                *elem = Luma([(fact_left * sum_left as f32 + fact_right * sum_right as f32) as u8]);
            } else if left != right {
                let fraction_vertical = (topf.fract() + bottomf.fract()) / 2.;
                let fract = fraction_vertical;

                let mut sum_bot = 0_u32;
                let mut sum_top = 0_u32;
                for x in left as usize..right as usize {
                    sum_bot += stack[[bottom as usize, x]] as u32;
                    sum_top += stack[[bottom as usize + 1, x]] as u32;
                }

                // Now we approximate: bot/n*fract + top/n*(1-fract)
                let fact_top = fract / ((right - left) as f32);
                let fact_bot = (1. - fract) / ((right - left) as f32);

                *elem = Luma([(fact_bot * sum_bot as f32 + fact_top * sum_top as f32) as u8]);
            } else {
                // bottom == top && left == right
                let fraction_horizontal = (topf.fract() + bottomf.fract()) / 2.;
                let fraction_vertical = (leftf.fract() + rightf.fract()) / 2.;

                let k_bl = stack[[bottom as usize, left as usize]];
                let k_tl = stack[[bottom as usize + 1, left as usize]];
                let k_br = stack[[bottom as usize, left as usize + 1]];
                let k_tr = stack[[bottom as usize + 1, left as usize + 1]];

                let frac_v = fraction_vertical;
                let frac_h = fraction_horizontal;

                let fact_tr = frac_v * frac_h;
                let fact_tl = frac_v * (1. - frac_h);
                let fact_br = (1. - frac_v) * frac_h;
                let fact_bl = (1. - frac_v) * (1. - frac_h);

                *elem = Luma([(fact_br * k_br as f32
                    + fact_tr * k_tr as f32
                    + fact_bl * k_bl as f32
                    + fact_tl * k_tl as f32) as u8])
            };
        });

    let out_file = out_dir.join(format!("{stem}.png"));
    image.save(&out_file)?;
    Ok(out_file.to_str().unwrap().to_string())
}

// /// Read an mono represented image. Currently assumes all data is a single byte
// fn blocked_read_mono(
//     &self,
//     data: &[u8],
//     block: &BlockInfo,
//     image: &mut RgbaImage,
// ) -> VizResult<()> {
//     // Make values outside of "significant" image data transparent
//     let alpha = |x: u32, y: u32| {
//         if x >= self.ncols || y >= self.nrows {
//             u8::MIN
//         } else {
//             u8::MAX
//         }
//     };

//     if self.nbpp != 8 {
//         return Err(VizError::Nbpp);
//     };

//     let mut block_iter = vec![(0_u32, 0_u32); (block.width * block.height) as usize];
//     for (i_y, y) in (block.y..(block.y + block.height)).enumerate() {
//         for (i_x, x) in (block.x..(block.x + block.width)).enumerate() {
//             block_iter[i_x + i_y * block.width as usize] = (x, y)
//         }
//     }

//     let block_iter = block_iter.iter().cloned();
//     for (data, (x, y)) in data.iter().zip(block_iter) {
//         image.put_pixel(x, y, Rgba([*data, *data, *data, alpha(x, y)]));
//     }

//     Ok(())
// }
// fn the_thing() {
//         let block_height = self.nppbv as u32;
//         let block_width = self.nppbh as u32;
//         let block_per_row = self.nbpr as u32;
//         let block_per_col = self.nbpc as u32;

//         let ncols = {
//             if block_per_row < 2 {
//                 self.ncols
//             } else {
//                 block_per_row * block_width
//             }
//         };
//         let nrows = {
//             if block_per_col < 2 {
//                 self.nrows
//             } else {
//                 block_per_col * block_height
//             }
//         };

//         let mut image = RgbaImage::new(ncols, nrows);

//         // If the image is not 'blocked',
//         if self.nbpr == 1 && self.nbpc == 1 {
//             match self.irep {
//                 ImageRepresentation::MONO => self.read_mono(&mut image),
//                 ImageRepresentation::RGB => self.read_rgb(&mut image),
//                 ImageRepresentation::RGBLUT => self.read_rgb_lut(&mut image),
//                 unimpl => Err(VizError::Irep(unimpl)),
//             }?;
//             return Ok(image);
//         }
//         let byte_per_px = (self.nbpp / 8) as u32;

//         let n_block = block_per_row * block_per_col;
//         let mut block_info = vec![BlockInfo::default(); n_block as usize];
//         for i_y in 0..block_per_col {
//             let y = i_y * block_height;
//             for i_x in 0..block_per_row {
//                 let x = i_x * block_width;
//                 let block_idx = i_x as usize + (i_y * block_per_row) as usize;
//                 block_info[block_idx] = BlockInfo {
//                     x,
//                     y,
//                     width: block_width,
//                     height: block_height,
//                 };
//             }
//         }
//         let chunk_size = (byte_per_px * block_width * block_height * self.nbands as u32) as usize;
//         let data_chunks = self.data.chunks_exact(chunk_size);
//     }
