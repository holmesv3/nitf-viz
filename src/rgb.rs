//! Definition of image reading/writing logic
use std::fs::File;

use image::{Rgb, RgbImage};

use ndarray::parallel::prelude::*;
use ndarray::ArrayView4;
use nitf_rs::headers::image_hdr::Mode;
use nitf_rs::Nitf;

use crate::handler::Handler;
use crate::VizResult;
use crate::{BlockedPaddedArray, Stack};

pub fn make_rgb(handler: Handler) -> VizResult<String> {
    let mut nitf_file = File::open(handler.input.clone())?;
    let nitf = Nitf::from_reader(&mut nitf_file)?;

    // Map out the full image from the individual segments
    let arrs: Vec<BlockedPaddedArray<[u8; 3]>> = nitf
        .image_segments
        .iter()
        .map(|s| {
            // n_rows and n_cols are the significant pixels, there may be fill data
            let n_rows = s.header.nrows.val as usize;
            let n_cols = s.header.ncols.val as usize;

            let mut block_height = s.header.nppbv.val as usize;
            let mut block_width = s.header.nppbh.val as usize;
            let mut block_per_row = s.header.nbpr.val as usize;
            let mut block_per_col = s.header.nbpc.val as usize;

            assert!(s.header.imode.val == Mode::P);

            if block_per_row <= 1 {
                block_per_row = 1;
                block_width = n_cols;
            }
            if block_per_col <= 1 {
                block_per_col = 1;
                block_height = n_rows;
            }

            let _mmap = s.get_data_map(&mut nitf_file).unwrap();
            let ptr = _mmap.as_ptr() as *const [u8; 3];
            let array = unsafe {
                ArrayView4::from_shape_ptr(
                    (block_per_col, block_per_row, block_height, block_width),
                    ptr,
                )
            };
            BlockedPaddedArray {
                array,
                n_rows,
                n_cols,
                _mmap,
            }
        })
        .collect();

    // This assumes all segments have the same number of columns, and are split along rows
    let n_rows = arrs.iter().fold(0, |acc, arr| acc + arr.n_rows as u32);
    let n_cols = arrs[0].n_cols as u32;

    let stack = Stack { arrs };

    let (out_rows, out_cols) = handler.calc_size(n_rows as f32, n_cols as f32);

    let x_ratio = n_cols as f32 / out_cols as f32;
    let y_ratio = n_rows as f32 / out_rows as f32;

    let mut image = RgbImage::new(out_cols, out_rows);

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
                let mut pixel = [0u8; 3];
                pixel.iter_mut().enumerate().for_each(|(idx, px)| {
                    let mut sum = 0_f32;
                    for i_row in bottom as usize..top as usize {
                        for i_col in left as usize..right as usize {
                            sum += stack[[i_row, i_col]][idx] as f32;
                        }
                    }
                    *px = (sum / n) as u8;
                });
                *elem = Rgb(pixel);
            } else if bottom != top {
                let fract = (leftf.fract() + rightf.fract()) / 2.;
                let fact_right = fract / ((top - bottom) as f32);
                let fact_left = (1. - fract) / ((top - bottom) as f32);

                let mut pixel = [0u8; 3];
                pixel.iter_mut().enumerate().for_each(|(idx, px)| {
                    let mut sum_left = 0_f32;
                    let mut sum_right = 0_f32;
                    for x in bottom as usize..top as usize {
                        sum_left += stack[[x, left as usize]][idx] as f32;
                        sum_right += stack[[x, left as usize + 1]][idx] as f32;
                    }
                    *px = (fact_left * sum_left + fact_right * sum_right) as u8;
                });
                *elem = Rgb(pixel);
            } else if left != right {
                let fract = (topf.fract() + bottomf.fract()) / 2.;
                let fact_top = fract / ((right - left) as f32);
                let fact_bot = (1. - fract) / ((right - left) as f32);

                let mut pixel = [0u8; 3];
                pixel.iter_mut().enumerate().for_each(|(idx, px)| {
                    let mut sum_bot = 0_f32;
                    let mut sum_top = 0_f32;
                    for x in left as usize..right as usize {
                        sum_bot += stack[[bottom as usize, x]][idx] as f32;
                        sum_top += stack[[bottom as usize + 1, x]][idx] as f32;
                    }
                    *px = (fact_top * sum_bot + fact_bot * sum_top) as u8;
                });
                *elem = Rgb(pixel);
            } else {
                // bottom == top && left == right
                let frac_h = (leftf.fract() + rightf.fract()) / 2.;
                let frac_v = (topf.fract() + bottomf.fract()) / 2.;

                let fact_tr = frac_v * frac_h;
                let fact_tl = frac_v * (1. - frac_h);
                let fact_br = (1. - frac_v) * frac_h;
                let fact_bl = (1. - frac_v) * (1. - frac_h);

                let mut pixel = [0u8; 3];
                pixel.iter_mut().enumerate().for_each(|(idx, px)| {
                    let k_bl = stack[[bottom as usize, left as usize]][idx] as f32;
                    let k_tl = stack[[bottom as usize + 1, left as usize]][idx] as f32;
                    let k_br = stack[[bottom as usize, left as usize + 1]][idx] as f32;
                    let k_tr = stack[[bottom as usize + 1, left as usize + 1]][idx] as f32;
                    *px = (fact_br * k_br + fact_tr * k_tr + fact_bl * k_bl + fact_tl * k_tl) as u8
                });
                *elem = Rgb(pixel);
            };
        });

    let out_file = &handler.out_dir.join(format!("{}.png", handler.stem));
    image.save(&out_file)?;
    Ok(out_file.to_str().unwrap().to_string())
}
