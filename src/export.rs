// TODO: image and svg export.
use crate::matrix::SquareMatrix;
use std::path::Path;
#[cfg(feature = "svg")]
use svg::Document;
#[cfg(feature = "svg")]
use svg::node::element::Rectangle;

#[cfg(feature = "image")]
use image::{DynamicImage, GrayImage, ImageFormat};

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

    let (needs_resample, block_size) = if n > side_len || n < side_len {
        let multiple = n as f32 / side_len as f32;
        (true, multiple)
    } else {
        (false, 1f32)
    };

    // viewBox: (min-x, min-y, width, height)
    // SVG uses a builder.
    let mut doc = Document::new().set("viewBox", (0, 0, n, n));

    // i == y
    for i in 0..n {
        // j == x
        for j in 0..n {
            // Check whether to resample
            let val = if needs_resample {
                // Have to resample.
                matrix.sample_matrix(i, j, n)
            } else {
                // Same size/clamped = read directly from the matrix
                *matrix.get(i, j)
            };

            if 0 == val {
                // Just cast everything to f32
                let rect = Rectangle::new()
                    .set("x", j as f32 * block_size)
                    .set("y", i as f32 * block_size)
                    .set("width", block_size)
                    .set("height", block_size);

                doc = doc.add(rect);
            }
        }
    }
    doc
}

// TODO: Test this to determine whether it actually works as I understand it to from this document:
// https://css-tricks.com/scale-svg/#the-viewbox-attribute
#[cfg(feature = "svg")]
pub fn render_svg_without_resampling(
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Document {
    const BLOCK_SIZE: usize = 1;
    let side_len = matrix.side_length();

    // TODO: determine whether to clamp the minimum to the side length
    // or to export an error.
    let n = match side_length {
        Some(length) => length,
        None => side_len,
    };

    let needs_resample = n > side_len || n < side_len;

    // viewBox: (min-x, min-y, width, height)
    // SVG uses a builder.
    let mut doc = Document::new().set("viewBox", (0, 0, side_len, side_len));
    // TODO: test to see whether preserveAspectRatio needs to be set.

    // i == y
    for i in 0..side_len {
        // j == x
        for j in 0..side_len {
            let val = *matrix.get(i, j);
            if 0 == val {
                // Just cast everything to f32
                let rect = Rectangle::new()
                    .set("x", j)
                    .set("y", i)
                    .set("width", BLOCK_SIZE)
                    .set("height", BLOCK_SIZE);

                doc = doc.add(rect);
            }
        }
    }

    // Per my understanding, this will scale the svg properly.
    if needs_resample {
        doc = doc.set("viewBox", (0, 0, n, n));
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
    // If this does not work, remove the function and swap with the resampling one.
    let svg = render_svg_without_resampling(matrix, side_length);
    write_svg(file_path, &svg)
}

#[inline]
#[cfg(feature = "svg")]
pub(crate) fn write_svg(file_path: &Path, svg: &Document) -> Result<(), String> {
    let res = svg::save(file_path, svg);
    res.map_err(|e| format!("ERROR: {}\nKIND: {}", e.to_string(), e.kind()))
}

// Same idea as the svg export -> but let this export a generic "image"
// Transform it into a png on png_save.
// Also, this -will- require resampling if side_length is Some(length)
// and length != matrix.side_length()
//
// Again, as mentioned above, it is ill advised to resize below the minimum size required
// for the given QR version.
#[cfg(feature = "image")]
pub fn render_image(matrix: &SquareMatrix<u8>, side_length: Option<usize>) -> DynamicImage {
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

    let (raw_bytes, _) = export_image.destructure_into_bytes();

    // TODO: test this, expect panics -> refactor panics out after testing.
    let img = GrayImage::from_raw(n as u32, n as u32, raw_bytes)
        .expect("The image buffer should map 1:1 with image::GrayImage");

    DynamicImage::ImageLuma8(img)
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
    // NOTE: THIS WILL (currently) PANIC IF THE IMAGE TRANSLATION WILL NOT WORK.
    let png = render_image(matrix, side_length);
    let format = ImageFormat::Png;
    png.save_with_format(file_path, format)
        // TODO: better error handling
        .map_err(|e| format!("ERROR: {}\nKIND: {}", e.to_string(), &e))
}
