use log::debug;
use ndarray::ArrayView2;
use rayon::prelude::*;

use crate::C32Layout;

fn amplitude(z: &C32Layout) -> f32 {
    let real = f32::from_be_bytes(z[0]);
    let imag = f32::from_be_bytes(z[1]);
    (real.powi(2) + imag.powi(2))
        .sqrt()
        .clamp(f32::MIN, f32::MAX)
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Pedf {
    eps: f32,
    slope: f32,
    constant: f32,
}

impl Pedf {
    fn density_call(&self, z: &C32Layout) -> f32 {
        self.slope * amplitude(z).max(self.eps) + self.constant
    }
    /// Given the raw pixel data, determine remap parameters for a callable remap
    pub fn init(buffer: &[u8], n_rows: impl Into<usize>, n_cols: impl Into<usize>) -> Self {
        debug!("Computing PEDF remap parameters");
        let n_rows: usize = n_rows.into();
        let n_cols: usize = n_cols.into();
        let dmin: f32 = 30.0;
        let mmult: f32 = 40.0;
        let n_elem = (n_rows * n_cols) as f32;

        let arr = unsafe {
            ArrayView2::from_shape_ptr((n_rows, n_cols), buffer.as_ptr() as *const C32Layout)
        };
        // Get the mean of the pixel data
        let mean: f32 = {
            arr.into_par_iter()
                .map(|z| {
                    let amp = amplitude(z);
                    match amp.is_finite() {
                        true => Some(amplitude(z)),
                        false => None,
                    }
                })
                .fold(
                    || 0_f32,
                    |a, b| {
                        if b.is_some() {
                            a + b.unwrap()
                        } else {
                            a
                        }
                    },
                )
                .sum::<f32>()
                / n_elem
        };

        dbg!(mean);

        let c_l = 0.8 * mean;
        let c_h = mmult * c_l;

        let slope = (u8::MAX as f32 - dmin) / (c_h / c_l).log10();
        let constant = dmin - slope * c_l.log10();

        Self {
            eps: 1E-5_f32,
            slope,
            constant,
        }
    }
    pub fn remap(&self, z: &C32Layout) -> u8 {
        let density_remap = self.density_call(z);
        let half = 127_f32;
        let out = {
            if density_remap <= half {
                density_remap
            } else {
                0.5 * (density_remap + half)
            }
        };
        out as u8
    }
}
