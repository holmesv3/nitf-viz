//! Definition of image reading/writing logic
use image::{
    imageops::colorops::{brighten_in_place, contrast_in_place},
    Rgba, RgbaImage,
};
use log::{debug, error, info};
use memmap2::Mmap;
use ndarray::{Array2, ArrayView2, Zip};
use nitf_rs::headers::image_hdr::*;
use nitf_rs::Nitf;
use rayon::prelude::*;
use sicd_rs::SicdMeta;
use std::{fs::File, ops::Index};

use crate::remap::Pedf;
use crate::{cli::Cli, remap::amplitude, C32Layout};
use crate::{VizError, VizResult};
struct StackedArrays {
    arrays: Vec<ArrayView2<'static, C32Layout>>,
    rows: Vec<u32>,
}

impl Index<[usize; 2]> for StackedArrays {
    type Output = C32Layout;
    fn index(&self, index: [usize; 2]) -> &Self::Output {
        let (i_arr, i_row) = self.arr_row_idx(index[0]);
        let i_col = index[1];
        &self.arrays[i_arr][[i_row, i_col]]
    }
}

impl StackedArrays {
    fn arr_row_idx(&self, i_row: usize) -> (usize, usize) {
        // Use a 'global' index into the image data
        for i_arr in 0..self.rows.len() {
            let sum = self.rows[..=i_arr].iter().sum::<u32>() as usize;
            if i_row < sum {
                let prior = self.rows[..i_arr].iter().sum::<u32>() as usize;
                return (i_arr, i_row - prior);
            }
        }
        panic!("INDEXING BROke CUZ")
    }
}

pub fn run(args: &Cli) -> VizResult<()> {
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

    if nitf.image_segments[0].header.imode.val == Mode::B {
        error!("WE CAN'T BE DOIONG THAT BLOCKED IMAGE MODE READING MR CRABS!!!!");
        return Err(VizError::DoBetter);
    };

    // Map out the full image  from the individual segments
    let rows: Vec<u32> = nitf
        .image_segments
        .iter()
        .map(|s| s.header.nrows.val)
        .collect();
    let cols: Vec<u32> = nitf
        .image_segments
        .iter()
        .map(|s| s.header.ncols.val)
        .collect();
    let maps: Vec<Mmap> = nitf
        .image_segments
        .iter()
        .map(|s| s.get_data_map(&mut nitf_file).unwrap())
        .collect();

    let arrays: Vec<ArrayView2<C32Layout>> = maps
        .iter()
        .zip(rows.clone())
        .zip(cols.clone())
        .map(|((m, n_row), n_col)| unsafe {
            ArrayView2::from_shape_ptr(
                (n_row as usize, n_col as usize),
                m.as_ptr() as *const C32Layout,
            )
        })
        .collect();

    debug!("Calculating remap parameters");
    let mean = arrays
        .iter()
        .map(|arr| {
            arr.into_par_iter().map(amplitude).sum::<f32>()
                / arr.shape().iter().product::<usize>() as f32
        })
        .sum::<f32>()
        / arrays.len() as f32;

    let dmin: f32 = 30.0;
    let mmult: f32 = 40.0;

    let c_l = 0.8 * mean;
    let c_h = mmult * c_l;

    let eps = 1E-5_f32;
    let slope = (u8::MAX as f32 - dmin) / (c_h / c_l).log10();
    let constant = dmin - slope * c_l.log10();

    let pedf = Pedf {
        eps,
        slope,
        constant,
    };
    let stack = StackedArrays {
        arrays,
        rows: rows.clone(),
    };

    let n_rows = rows.iter().sum::<u32>();
    let n_cols = cols[0];

    debug!("Creating image");
    // Determine the input aspect ratio and chunk size
    let sicd = sicd_rs::read_sicd(&args.input).unwrap();
    let (row_ss, col_ss, graze, twist) = match sicd.meta {
        SicdMeta::V0_4_0(m) => (
            m.grid.row.ss,
            m.grid.col.ss,
            m.scpcoa.graze_ang.to_radians(),
            m.scpcoa.twist_ang.to_radians(),
        ),
        SicdMeta::V0_5_0(m) => (
            m.grid.row.ss,
            m.grid.col.ss,
            m.scpcoa.graze_ang.to_radians(),
            m.scpcoa.twist_ang.to_radians(),
        ),
        SicdMeta::V1(m) => (
            m.grid.row.ss,
            m.grid.col.ss,
            m.scpcoa.graze_ang.to_radians(),
            m.scpcoa.twist_ang.to_radians(),
        ),
        _ => return Err(VizError::DoBetter),
    };

    let row_res = (row_ss / graze.cos()).abs();
    let col_res =
        ((graze.tan() * twist.tan() * row_ss).powi(2) + (col_ss / twist.cos()).powi(2)).sqrt();

    debug!("Found resolution {row_res} X {col_res}");

    // let aspect = (n_cols as f64 ) / (n_rows as f64 );
    let aspect = (n_cols as f64 * col_res) / (n_rows as f64 * row_res);
    debug!("Input aspect ratio: {aspect} : 1");
    debug!("Original dimensions: {} X {}", n_rows, n_cols);

    let max_size = size.pow(2) as f64;
    let out_cols = (aspect * max_size).sqrt() as u32;
    let out_rows = (max_size / out_cols as f64) as u32;
    debug!("Thumbnail dimensions: {out_rows} X {out_cols}");

    let mut out = Array2::zeros((out_rows as usize, out_cols as usize));
    let x_ratio = n_cols as f32 / out_cols as f32;
    let y_ratio = n_rows as f32 / out_rows as f32;

    Zip::indexed(&mut out).par_for_each(|(outy, outx), elem| {
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
                    res += pedf.remap(&stack[[i_row, i_col]]) as f32
                }
            }
            *elem = (res / n) as u8;
        } else if bottom != top {
            let fract = (leftf.fract() + rightf.fract()) / 2.;

            let mut sum_left = 0_u32;
            let mut sum_right = 0_u32;
            for x in bottom as usize..top as usize {
                sum_left += pedf.remap(&stack[[x, left as usize]]) as u32;
                sum_right += pedf.remap(&stack[[x, left as usize + 1]]) as u32;
            }

            // Now we approximate: left/n*(1-fract) + right/n*fract
            let fact_right = fract / ((top - bottom) as f32);
            let fact_left = (1. - fract) / ((top - bottom) as f32);

            *elem = (fact_left * sum_left as f32 + fact_right * sum_right as f32) as u8;
        } else if left != right {
            let fraction_vertical = (topf.fract() + bottomf.fract()) / 2.;
            let fract = fraction_vertical;

            let mut sum_bot = 0_u32;
            let mut sum_top = 0_u32;
            for x in left as usize..right as usize {
                sum_bot += pedf.remap(&stack[[bottom as usize, x]]) as u32;
                sum_top += pedf.remap(&stack[[bottom as usize + 1, x]]) as u32;
            }

            // Now we approximate: bot/n*fract + top/n*(1-fract)
            let fact_top = fract / ((right - left) as f32);
            let fact_bot = (1. - fract) / ((right - left) as f32);

            *elem = (fact_bot * sum_bot as f32 + fact_top * sum_top as f32) as u8;
        } else {
            // bottom == top && left == right
            let fraction_horizontal = (topf.fract() + bottomf.fract()) / 2.;
            let fraction_vertical = (leftf.fract() + rightf.fract()) / 2.;

            let k_bl = pedf.remap(&stack[[bottom as usize, left as usize]]);
            let k_tl = pedf.remap(&stack[[bottom as usize + 1, left as usize]]);
            let k_br = pedf.remap(&stack[[bottom as usize, left as usize + 1]]);
            let k_tr = pedf.remap(&stack[[bottom as usize + 1, left as usize + 1]]);

            let frac_v = fraction_vertical;
            let frac_h = fraction_horizontal;

            let fact_tr = frac_v * frac_h;
            let fact_tl = frac_v * (1. - frac_h);
            let fact_br = (1. - frac_v) * frac_h;
            let fact_bl = (1. - frac_v) * (1. - frac_h);

            *elem = (fact_br * k_br as f32
                + fact_tr * k_tr as f32
                + fact_bl * k_bl as f32
                + fact_tl * k_tl as f32) as u8
        };
    });

    let mut image = RgbaImage::new(out_cols, out_rows);
    out.iter()
        .cloned()
        .zip(image.pixels_mut())
        .for_each(|(data, px)| {
            *px = Rgba([data, data, data, u8::MAX]);
        });

    if args.brightness != 0 {
        debug!("Adjusting brightness");
        brighten_in_place(&mut image, args.brightness);
    }
    if args.contrast != 0.0 {
        debug!("Adjusting contrast");
        contrast_in_place(&mut image, args.contrast);
    }

    let out_file = out_dir.join(format!("{}_{}.png", stem, size));
    image.save(&out_file)?;
    info!("Finished writing {}", out_file.to_str().unwrap());
    Ok(())
}
