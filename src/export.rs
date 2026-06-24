// TODO: image and svg export.
use crate::matrix::SquareMatrix;
use std::path::Path;
#[cfg(feature = "svg")]
use svg::Document;
#[cfg(feature = "svg")]
use svg::node::element::Rectangle;

#[cfg(feature = "image")]
use image::{DynamicImage, GrayImage, ImageFormat, Luma};

// TODO: clean up the png export to use proper filtering to handle scaling situations.

// Basic resize/resampling functions

// TODO: work this into a proper docstring.
// For best results, the new side length should be an integer multiple of the original matrix side
// length (i.e. use this to perform integer scaling)
// It is ill-advised to resize to a size smaller than the minimum required side length for the QR
// It is also ill-advised to resize a resized QR code. Since this function returns a new
// SquareMatrix<u8> the original will not be dropped--resize from the original matrix
// returned from QRCodeMatrix::render().
// If you require fine-control over scaling, prefer exporting to svg.
//
// The minimum side length, s: (((v-1) * 4) + 21), where v is the version (counting from 1)
// TODO: guard against this; abstract over SquareMatrix<u8> and append the QR version.
// TODO TWICE: refactor this to return a result if the new side length is smaller than the
// original.
#[cfg(any(feature = "svg", feature = "image"))]
pub fn resize(matrix: &SquareMatrix<u8>, new_side_length: usize) -> SquareMatrix<u8> {
    let mut out_mat = SquareMatrix::new(new_side_length);
    for i in 0..new_side_length {
        for j in 0..new_side_length {
            *out_mat.get_mut(i, j) = matrix.sample_matrix(i, j, new_side_length);
        }
    }

    out_mat
}

// This is a convenience function to avoid fractional scaling
// Negative numbers indicate "divide" the old len by the absolute value of this number.
// Positive numbers indicate "multiply" the old len by this number.
#[inline]
#[cfg(any(feature = "svg", feature = "image"))]
pub fn nearest_integer_multiple(old_side_length: usize, new_side_length: usize) -> i32 {
    if new_side_length < old_side_length {
        -((old_side_length as f32 / new_side_length as f32).ceil() as i32)
    } else if new_side_length > old_side_length {
        (new_side_length as f32 / old_side_length as f32).ceil() as i32
    } else {
        1
    }
}

// MDN SVG: https://developer.mozilla.org/en-US/docs/Web/SVG/Reference/Attribute/viewBox
// MDN SVG: https://developer.mozilla.org/en-US/docs/Web/SVG/Reference/Element/rect

// NOTE: it is ill-advised to resample the pixels to a size smaller than
// the minimum size per version:
// The minimum side length, s: (((v-1) * 4) + 21), where v is the version (counting from 1)
// TODO: (per above) guard against this; abstract over SquareMatrix<u8> and append the QR version.
// TODO TWICE: refactor this to return a result if the new side length is smaller than the
// original. -> or, get rid of this function if it's handled by svg scaling.
// (again, ill advised to scale the svg below the pixel size -> but svgs are infinitely scalable)
//
// Loop through each cell, on a black one, set x (j) and y (i),
// width and height are the same length -> use 1 to make it easier; svgs are scalable.
// desired side_length = desired width = desired height
//
// NOTE: I'm not entirely sure whether it's possible to write the svg doc and then scale it to the
// desired size afterward by resetting the viewbox.
// ====> I believe it might actually be possible to do that.
// If the viewBox resize will automatically scale the svg without issue, then remove this function.
// TODO: consider removing this entirely -> it's
#[cfg(feature = "svg")]
pub fn render_svg_with_resampling(
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Document {
    let side_len = matrix.side_length();

    let n = match side_length {
        Some(length) => length,
        None => side_len,
    };

    let needs_resample = n != side_len;
    let bg = Rectangle::new()
        .set("x", 0)
        .set("y", 0)
        .set("width", n)
        .set("height", n)
        .set("fill", "white");

    // viewBox: (min-x, min-y, width, height)
    // SVG uses a builder.
    let mut doc = Document::new().set("viewBox", (0, 0, n, n)).add(bg);

    // i == y
    for i in 0..n {
        // j == x
        for j in 0..n {
            // Check whether to resample
            let val = if needs_resample {
                // Here we resample and just write more square units.
                matrix.sample_matrix(i, j, n)
            } else {
                // Same size/clamped = read directly from the matrix
                *matrix.get(i, j)
            };

            if 0 == val {
                let rect = Rectangle::new()
                    .set("x", j)
                    .set("y", i)
                    .set("width", 1)
                    .set("height", 1)
                    .set("fill", "black")
                    .set("stroke", 0.5);

                doc = doc.add(rect);
            }
        }
    }
    doc
}

// Prefer this function -> it's more accurate and can handle fractional scaling.
#[cfg(feature = "svg")]
pub fn render_svg_without_resampling(
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Document {
    const BLOCK_SIZE: usize = 1;
    let side_len = matrix.side_length();

    let n = match side_length {
        Some(length) => length,
        None => side_len,
    };

    let needs_resample = n != side_len;

    let block_size = if needs_resample {
        n as f32 / side_len as f32
    } else {
        1.0
    };

    let bg = Rectangle::new()
        .set("x", 0)
        .set("y", 0)
        .set("width", n)
        .set("height", n)
        .set("fill", "white");
    // viewBox: (min-x, min-y, width, height)
    // SVG uses a builder.
    let mut doc = Document::new().set("viewBox", (0, 0, n, n)).add(bg);
    // i == y
    for i in 0..side_len {
        // j == x
        for j in 0..side_len {
            let val = *matrix.get(i, j);
            if 0 == val {
                // Just cast everything to f32
                let rect = Rectangle::new()
                    .set("x", j as f32 * block_size)
                    .set("y", i as f32 * block_size)
                    .set("width", block_size)
                    .set("height", block_size)
                    .set("fill", "black")
                    .set("stroke", 0.5);

                doc = doc.add(rect);
            }
        }
    }

    doc
}

// Supply None to just use the matrix.side_length()
// TODO: implement actual error types and return a proper error
// TODO TWICE: (see above re: resizing issues).
#[cfg(feature = "svg")]
pub fn save_svg(
    file_path: &Path,
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Result<(), String> {
    // This function seems to be more correct.
    let svg = render_svg_without_resampling(matrix, side_length);
    write_svg(file_path, &svg)
}

#[inline]
#[cfg(feature = "svg")]
pub(crate) fn write_svg(file_path: &Path, svg: &Document) -> Result<(), String> {
    let res = svg::save(file_path, svg);
    res.map_err(|e| format!("ERROR: {}\nKIND: {}", e.to_string(), e.kind()))
}

// TODO: better docstring.
// If fractional scaling is desired/necessary, as well as a DynamicImage,
// the image should be rendered to the highest, closest, integer multiple,
// then the remaining should be done via image resizing with the returned DynamicImage
// I am not going to make the decision there.
//
// (DynamicImage, is_fract: bool), is_fract indicates whether fractional scaling was detected.
// At the moment, this does not round up to the nearest integer multiple.
//
// Same idea as the svg export -> but let this export a generic "image"
// Transform it into a png on png_save.
// Also, this -will- require resampling if side_length is Some(length)
// and length != matrix.side_length()
//
// TODO: Refactor this -> the resampling isn't all that correct I don't think.
// prefer the interpolation available in the image crate.
// Nearest-neighbor will work best for integer multiples
// Bilinear (?) for fractional scales.
//
// Again, as mentioned above, it is ill advised to resize below the minimum size required
// for the given QR version.
#[cfg(feature = "image")]
pub fn render_image(matrix: &SquareMatrix<u8>, side_length: Option<usize>) -> (DynamicImage, bool) {
    let side_len = matrix.side_length();
    let n = match side_length {
        Some(length) => length,
        None => side_len,
    };

    // Destructuring the SquareMatrix consumes it, so this has to clone
    // the borrow.

    // If the size is not the same size as the old side_len, the matrix has to be resized
    let export_image = if n != side_len {
        resize(matrix, n)
    } else {
        matrix.clone()
    };

    let mut img = GrayImage::new(n as u32, n as u32);

    // The buffer has to be written, since this the assumption is 8bits per pixel.
    // It thus has an r, g, and b channel.
    for i in 0..n {
        for j in 0..n {
            let val = *export_image.get(i, j);
            // Multiply it by 255, black will cancel this out.
            let color = val * 0xFF;
            *img.get_pixel_mut(j as u32, i as u32) = Luma([color]);
        }
    }

    let is_fract = if n > side_len {
        n.rem_euclid(side_len) > 0
    } else {
        side_len.rem_euclid(n) > 0
    };

    #[cfg(debug_assertions)]
    {
        if is_fract {
            eprintln!("Likely fractional: n: {n}, old_len: {side_len}");
        }
    }

    (DynamicImage::ImageLuma8(img), is_fract)
}

// This api will require the resampling algorithm to resample pixels up to a larger size.
// Png/Tiff/Bmp are likely the best candidates for this kind of export, since QR works best with pixel
// precision and zero anti-aliasing.
//
// For now, this will only expose png.
// Users who wish to export in another format can use render_image() and work with
// the returned DynamicImage (Luma 8).
#[cfg(feature = "image")]
pub fn save_png(
    file_path: &Path,
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Result<(), String> {
    let (png, _likely_fract) = render_image(matrix, side_length);
    let format = ImageFormat::Png;

    png.save_with_format(file_path, format)
        // TODO: better error handling
        .map_err(|e| format!("ERROR: {}\nKIND: {}", e.to_string(), &e))
}

// TODO: this should -probably- not use the sample_matrix method.
// This will take the provided side length and round it toward to the nearest integer multiple.
#[cfg(feature = "image")]
pub fn save_png_integer_scaling(
    file_path: &Path,
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Result<(), String> {
    let old_side_length = matrix.side_length();
    let modified_len = side_length.and_then(|length| {
        let mul = nearest_integer_multiple(old_side_length, length);
        let new_len = if mul > 0 {
            old_side_length * mul as usize
        } else {
            old_side_length / (mul.abs() as usize)
        };
        Some(new_len)
    });

    let (png, likely_fract) = render_image(matrix, modified_len);
    // TODO: results.
    assert!(
        !likely_fract,
        "The integer scaling should not cause fractional scaling."
    );
    let format = ImageFormat::Png;

    png.save_with_format(file_path, format)
        // TODO: better error handling
        .map_err(|e| format!("ERROR: {}\nKIND: {}", e.to_string(), &e))
}
