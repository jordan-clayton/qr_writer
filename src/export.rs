// TODO: image and svg export.
use crate::matrix::SquareMatrix;
use std::path::Path;
#[cfg(feature = "svg")]
use svg::Document;
#[cfg(feature = "svg")]
use svg::node::element::Rectangle;

// MDN SVG: https://developer.mozilla.org/en-US/docs/Web/SVG/Reference/Attribute/viewBox
// MDN SVG: https://developer.mozilla.org/en-US/docs/Web/SVG/Reference/Element/rect

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

    // TODO: determine whether to clamp the minimum to the side length
    // or to export an error.
    let n = match side_length {
        Some(length) => length.max(side_len),
        None => side_len,
    };

    // Fractional sampling is allowed, I think?
    let (needs_resample, block_size) = if n > side_len {
        // Take the nearest integer multiple
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
        Some(length) => length.max(side_len),
        None => side_len,
    };

    let needs_resample = n > side_len;

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

    if needs_resample {
        doc = doc.set("viewBox", (0, 0, n, n));
    }

    doc
}

// NOTE: side_length has to be at least matrix.side_length() in ("pixel") length.
// Supply None to just use the matrix.side_length()
// TODO: implement actual error types.
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
// Also, this -will- require resampling if side_length is Some(length) and length >
// matrix.side_length()
// TODO: read image crate and determine best general return type.
#[cfg(feature = "image")]
pub fn render_image(matrix: &SquareMatrix<u8>, side_length: Option<usize>) {
    todo!("Implement image rendering.");
}

// This api will require the resampling algorithm to resample pixels up to a larger size.
// TODO: determine what formats to expose for basic export
#[cfg(feature = "image")]
pub fn save_png() {
    todo!("Implement png export");
}
