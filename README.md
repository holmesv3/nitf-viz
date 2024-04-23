# nitv
NITF Visualizer (`nitv`) is a program which will read a `nitf` file and attempt to create a `png` or `gif` from the image data

For questions, feature requests, or bugs, please open an issue.

## Usage
First, install from clone or directly using `cargo`...
```sh
cargo install nitv
```
... then provide a NITF file
```sh
nitv <path-to-nitf>
```
There are a handful of options available
```
--output      Output folder [default: .]
--prefix      Output file name. Derived from input if not given
--size        sqrt(num-pixels) e.g., --size 50 -> 50^2 pixel image [default: 256]
--brightness  Adjust the brightness of the image product (32-bit signed integer) [default: 0]
--contrast    Adjust the contrast of the image product (32-bit float) [default: 0]
--level       Log level [default: info] [possible values: off, error, warn, info, debug, trace]
--nitf-log    Enable logging for nitf reading
```

## Current support (files from [Umbra's Open Data](https://umbra.space/open-data/))
### SIDD / monochrome
![SIDD product example](./images/sidd.png =256x256)
### RGB/RGB + LUT
![RGB product example](./images/rgb.png =256x256)
### SICD / complex-data
![SICD product example](./images/sicd.png =256x256)


## Implementation details:
The determination of whether to make a PNG or GIF is currently somewhat `hacky`.

Because SICD files can have image data spread across multiple segments, that processing logic is unique, thus the first thing which is done is to determine if the file contains SICD metadata.
- If it is determined to be a SICD, all image data is piecewise extended density format (PEDF) remapped, ground projected, and rendered to a PNG.
- If it doesn't contain SICD metadata but has multiple image segments, the data from each segment is rendered as a frame in a GIF.
- If it doesn't contain SICD metadata and has a single image segment, it is rendered as a PNG.

As more features are added, this logic will become more `sophisticated`