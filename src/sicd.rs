//! SICD specific image creation

//! Definition of image reading/writing logic
use image::{GrayImage, Luma};
use log::{debug, error};
use memmap2::Mmap;
use ndarray::ArrayView2;
use nitf_rs::headers::image_hdr::*;
use nitf_rs::Nitf;
use rayon::prelude::*;

use core::str;
use quick_xml::{events::Event, Reader};
use std::{fs::File, ops::Index};

use crate::handler::Handler;
use crate::{VizError, VizResult};

type C32Layout = [[u8; 4]; 2];

pub fn amplitude(z: &C32Layout) -> f32 {
    let real = f32::from_be_bytes(z[0]);
    let imag = f32::from_be_bytes(z[1]);
    (real.powi(2) + imag.powi(2))
        .sqrt()
        .clamp(f32::MIN, f32::MAX)
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Pedf {
    pub eps: f32,
    pub slope: f32,
    pub constant: f32,
}

impl Pedf {
    fn density_call(&self, z: &C32Layout) -> f32 {
        self.slope * amplitude(z).max(self.eps).log10() + self.constant
    }

    pub fn remap(&self, z: &C32Layout) -> u8 {
        let density_remap = self.density_call(z);
        let half = (u8::MAX / 2) as f32;

        let out = if density_remap <= half {
            density_remap
        } else {
            0.5 * (density_remap + half)
        };
        out as u8
    }
}

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

pub fn make_sicd(handler: Handler) -> VizResult<String> {
    debug!("Reading {:}", handler.input.to_str().unwrap());
    let mut nitf_file = File::open(handler.input.clone())?;
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
    let (row_ss, col_ss, graze, twist) =
        xml::read_xml(&nitf.data_extension_segments[0].get_data_map(&mut nitf_file)?[..])?;

    let row_res = (row_ss / graze.to_radians().cos()).abs();
    let col_res = ((graze.to_radians().tan() * twist.to_radians().tan() * row_ss).powi(2)
        + (col_ss / twist.to_radians().cos()).powi(2))
    .sqrt();

    debug!("SICD parameters: ");
    debug!("\t Grid.Row.SS= {row_ss}");
    debug!("\t Grid.Col.SS = {col_ss}");
    debug!("\t SCPCOA.GrazeAng = {}", graze.to_radians());
    debug!("\t SCPCOA.TwistAng = {}", twist.to_radians());
    debug!("Found SICD resolution {row_res} X {col_res}");

    let (out_rows, out_cols) = handler.calc_size(
        (n_rows as f64 * row_res) as f32,
        (n_cols as f64 * col_res) as f32,
    );
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

            let bottom = (bottomf.ceil() as u32).clamp(0, n_rows - 1) as usize;
            let top = topf.ceil().clamp(bottom as f32, n_rows as f32) as usize;
            let leftf = outx as f32 * x_ratio;
            let rightf = leftf + x_ratio;

            let left = leftf.ceil().clamp(0_f32, (n_cols - 1) as f32) as usize;
            let right = rightf.ceil().clamp(left as f32, n_cols as f32) as usize;

            if bottom != top && left != right {
                let n = ((top - bottom) * (right - left)) as f32;
                let mut res = 0_f32;
                for i_row in bottom..top {
                    for i_col in left..right {
                        res += pedf.remap(&stack[[i_row, i_col]]) as f32
                    }
                }
                *elem = Luma([(res / n) as u8]);
            } else if bottom != top {
                let fract = (leftf.fract() + rightf.fract()) / 2.;

                let mut sum_left = 0_u32;
                let mut sum_right = 0_u32;
                for x in bottom..top {
                    sum_left += pedf.remap(&stack[[x, left]]) as u32;
                    sum_right += pedf.remap(&stack[[x, left + 1]]) as u32;
                }

                // Now we approximate: left/n*(1-fract) + right/n*fract
                let fact_right = fract / ((top - bottom) as f32);
                let fact_left = (1. - fract) / ((top - bottom) as f32);

                *elem = Luma([(fact_left * sum_left as f32 + fact_right * sum_right as f32) as u8]);
            } else if left != right {
                let fract = (topf.fract() + bottomf.fract()) / 2.;

                let mut sum_bot = 0_u32;
                let mut sum_top = 0_u32;
                for x in left..right {
                    sum_bot += pedf.remap(&stack[[bottom, x]]) as u32;
                    sum_top += pedf.remap(&stack[[bottom + 1, x]]) as u32;
                }

                // Now we approximate: bot/n*fract + top/n*(1-fract)
                let fact_top = fract / ((right - left) as f32);
                let fact_bot = (1. - fract) / ((right - left) as f32);

                *elem = Luma([(fact_bot * sum_bot as f32 + fact_top * sum_top as f32) as u8]);
            } else {
                // bottom == top && left == right

                let k_bl = pedf.remap(&stack[[bottom, left]]);
                let k_tl = pedf.remap(&stack[[bottom + 1, left]]);
                let k_br = pedf.remap(&stack[[bottom, left + 1]]);
                let k_tr = pedf.remap(&stack[[bottom + 1, left + 1]]);

                let frac_v = (topf.fract() + bottomf.fract()) / 2.;
                let frac_h = (leftf.fract() + rightf.fract()) / 2.;

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

    let out_file = handler.out_dir.join(format!("{}.png", handler.stem));
    image.save(&out_file)?;
    Ok(out_file.to_str().unwrap().to_string())
}

/// Utility for reading SICD xml data
mod xml {
    use super::*;
    /// Get the projection values we need
    ///
    /// Instead of using `serde` approach which can fail for malformed data, this
    /// 'manual' approach should always work as long as the xml data is ok
    pub fn read_xml(xml: &[u8]) -> VizResult<(f64, f64, f64, f64)> {
        let mut reader = Reader::from_reader(xml);
        let e = reader.read_event()?;

        // If the first event we get isn't the SICD tag, something is wrong
        if let Event::Start(b) = e {
            if !str::from_utf8(b.name().0)?.contains("SICD") {
                return Err(VizError::DoBetter);
            }
        }

        // Prealloc variables
        let mut row_ss = 0_f64;
        let mut col_ss = 0_f64;
        let mut graze_ang = 0_f64;
        let mut twist_ang = 0_f64;

        // Now that we've made it here, we can iterate over the xml and find what we want
        let mut found_grid = false;
        let mut found_scpcoa = false;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        b"Grid" => found_grid = read_grid(&mut reader, &mut row_ss, &mut col_ss)?,
                        b"SCPCOA" => {
                            found_scpcoa = read_scpcoa(&mut reader, &mut graze_ang, &mut twist_ang)?
                        }
                        _ => (),
                    }
                    // No matter what start we find, read to the end of it, then read the "Event::End()" event
                    reader.read_to_end(e.to_end().name())?;
                }
                _ => return Err(VizError::DoBetter),
            }
            if found_grid && found_scpcoa {
                break;
            }
        }

        Ok((row_ss, col_ss, graze_ang, twist_ang))
    }

    fn read_grid(
        reader: &mut Reader<&[u8]>,
        row_ss: &mut f64,
        col_ss: &mut f64,
    ) -> VizResult<bool> {
        let mut found_row = false;
        let mut found_col = false;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        b"Row" => found_row = read_ss(reader, row_ss)?,
                        b"Col" => found_col = read_ss(reader, col_ss)?,
                        // If the element isn't the row or column group, skip it
                        _ => (),
                    }
                    // No matter what start we find, read to the end of it, then read the "Event::End()" event
                    reader.read_to_end(e.to_end().name())?;
                    if found_row && found_col {
                        break Ok(true);
                    }
                }
                _ => return Err(VizError::DoBetter),
            }
        }
    }

    fn read_ss(reader: &mut Reader<&[u8]>, val: &mut f64) -> VizResult<bool> {
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    if e.name().as_ref() == b"SS" {
                        break read_float(reader, val);
                    } else {
                        reader.read_to_end(e.to_end().name())?;
                    }
                }
                _ => return Err(VizError::DoBetter),
            }
        }
    }

    fn read_scpcoa(
        reader: &mut Reader<&[u8]>,
        graze_ang: &mut f64,
        twist_ang: &mut f64,
    ) -> VizResult<bool> {
        let mut found_graze = false;
        let mut found_twist = false;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    match e.name().as_ref() {
                        b"GrazeAng" => found_graze = read_float(reader, graze_ang)?,
                        b"TwistAng" => found_twist = read_float(reader, twist_ang)?,
                        _ => (),
                    };
                    // No matter what start we find, read to the end of it, then read the "Event::End()" event
                    reader.read_to_end(e.to_end().name())?;
                    if found_graze && found_twist {
                        break Ok(true);
                    }
                }
                _ => return Err(VizError::DoBetter),
            }
        }
    }

    fn read_float(reader: &mut Reader<&[u8]>, val: &mut f64) -> VizResult<bool> {
        match reader.read_event() {
            Ok(Event::Text(txt)) => {
                *val = str::from_utf8(txt.as_ref())?.parse().unwrap();
                Ok(true)
            }
            _ => Err(VizError::DoBetter),
        }
    }
}
