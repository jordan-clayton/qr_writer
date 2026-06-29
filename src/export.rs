use crate::matrix::SquareMatrix;
use std::path::Path;
#[cfg(feature = "svg")]
use svg::Document;
#[cfg(feature = "svg")]
use svg::node::element::Rectangle;

#[cfg(any(feature = "image", feature = "png"))]
use image::{DynamicImage, GrayImage, ImageFormat, Luma};

use crate::errors::{ArithmeticError, QrError, Result};

pub enum IntegerInverse {
    Divide(usize),
    Multiply(usize),
}

// This is a convenience function to avoid fractional scaling
#[inline]
pub fn nearest_integer_multiple(
    old_side_length: usize,
    new_side_length: usize,
) -> Result<IntegerInverse> {
    if new_side_length < old_side_length {
        if new_side_length == 0 {
            Err(QrError::ArithmeticError(ArithmeticError::ZeroDivision))
        } else {
            let n = (old_side_length as f32 / new_side_length as f32).ceil() as usize;

            Ok(IntegerInverse::Divide(n))
        }
    } else if new_side_length > old_side_length {
        if old_side_length == 0 {
            Err(QrError::ArithmeticError(ArithmeticError::ZeroDivision))
        } else {
            let n = (new_side_length as f32 / old_side_length as f32).ceil() as usize;
            Ok(IntegerInverse::Multiply(n))
        }
    } else {
        if new_side_length == 0 {
            Err(QrError::ArithmeticError(ArithmeticError::ZeroDivision))
        } else {
            Ok(IntegerInverse::Multiply(1))
        }
    }
}

// --------- SVG ----------

#[cfg(feature = "svg")]
#[derive(Copy, Clone, Default)]
pub struct SvgHints<'a> {
    pub background_color: Option<&'a str>,
    // Use "crispEdges" to turn off subpixel rendering.
    pub shape_rendering: Option<&'a str>,
    pub pixel_hints: Option<SvgRectHints<'a>>,
}

#[cfg(feature = "svg")]
#[derive(Copy, Clone, Default)]
pub struct SvgRectHints<'a> {
    pub background_color: Option<&'a str>,
    pub stroke: Option<Stroke<'a>>,
    pub rx: Option<&'a str>,
    pub ry: Option<&'a str>,
}

#[cfg(feature = "svg")]
#[derive(Copy, Clone, Default)]
pub struct Stroke<'a> {
    pub color: Option<&'a str>,
    pub width: Option<&'a str>,
}

// MDN SVG: https://developer.mozilla.org/en-US/docs/Web/SVG/Reference/Attribute/viewBox
// MDN SVG: https://developer.mozilla.org/en-US/docs/Web/SVG/Reference/Element/rect

// SVGs are infinitely scalable -> but with nearest-neighbor resampling,
// sampling below the minimum pixel size will degrade the QR
// TODO guard against this if the function is going to remain.
//
// Loop through each cell, on a black one, set x (j) and y (i),
// width and height are the same length -> use 1 to make it easier; svgs are scalable.
// desired side_length = desired width = desired height
#[cfg(feature = "svg")]
pub fn render_svg_with_resampling(
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
    hints: Option<SvgHints>,
) -> Result<Document> {
    let side_len = matrix.side_length();

    let n = match side_length {
        Some(length) => length,
        None => side_len,
    };

    let hints = hints.unwrap_or_default();

    let bg_color = hints.background_color.unwrap_or("white");

    let needs_resample = n != side_len;
    let mut bg = Rectangle::new()
        .set("x", 0)
        .set("y", 0)
        .set("width", n)
        .set("height", n)
        .set("fill", bg_color);

    if let Some(shape_rendering) = hints.shape_rendering {
        bg = bg.set("shape-rendering", shape_rendering);
    }

    // viewBox: (min-x, min-y, width, height)
    // SVG uses a builder.
    let mut doc = Document::new().set("viewBox", (0, 0, n, n)).add(bg);

    // unpack the rect hints.
    let pixel_hints = hints.pixel_hints.unwrap_or_default();
    let p_bg_col = pixel_hints.background_color.unwrap_or("black");

    // i == y
    for i in 0..n {
        // j == x
        for j in 0..n {
            // Check whether to resample
            let val = if needs_resample {
                // Here we resample and just write more square units.
                matrix.sample_matrix(i, j, n)?
            } else {
                // Same size/clamped = read directly from the matrix
                *matrix.get(i, j).ok_or(QrError::RenderError {
                    reason: format!("Invalid read at i: {i}, j: {j} during svg render."),
                })?
            };

            if 0 == val {
                let mut rect = Rectangle::new()
                    .set("x", j)
                    .set("y", i)
                    .set("width", 1)
                    .set("height", 1)
                    .set("fill", p_bg_col);

                // Handle additional optional features:
                if let Some(rx) = pixel_hints.rx {
                    rect = rect.set("rx", rx);
                }
                if let Some(ry) = pixel_hints.ry {
                    rect = rect.set("ry", ry);
                }

                if let Some(shape_rendering) = hints.shape_rendering {
                    rect = rect.set("shape-rendering", shape_rendering);
                }

                if let Some(stroke_hints) = pixel_hints.stroke {
                    if let Some(width) = stroke_hints.width {
                        rect = rect.set("stroke-width", width);
                    }
                    if let Some(color) = stroke_hints.color {
                        rect = rect.set("stroke", color);
                    }
                }

                doc = doc.add(rect);
            }
        }
    }
    Ok(doc)
}

// Prefer this function -> it's more accurate and can handle fractional scaling.
// TODO: expose rounded-corner border radius.
#[cfg(feature = "svg")]
pub fn render_svg_without_resampling(
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
    hints: Option<SvgHints>,
) -> Result<Document> {
    let old_side_length = matrix.side_length();

    let n = match side_length {
        Some(new_len) => new_len,
        None => old_side_length,
    };

    let hints = hints.unwrap_or_default();

    let bg_color = hints.background_color.unwrap_or("white");

    let needs_resample = n != old_side_length;

    let block_size = if needs_resample {
        n as f32 / old_side_length as f32
    } else {
        1.0
    };

    let mut bg = Rectangle::new()
        .set("x", 0)
        .set("y", 0)
        .set("width", n)
        .set("height", n)
        .set("fill", bg_color);

    if let Some(shape_rendering) = hints.shape_rendering {
        bg = bg.set("shape-rendering", shape_rendering);
    }

    // unpack the rect hints.
    let pixel_hints = hints.pixel_hints.unwrap_or_default();
    let p_bg_col = pixel_hints.background_color.unwrap_or("black");

    // viewBox: (min-x, min-y, width, height)
    // SVG uses a builder.
    let mut doc = Document::new().set("viewBox", (0, 0, n, n)).add(bg);
    // i == y
    for i in 0..old_side_length {
        // j == x
        for j in 0..old_side_length {
            let val = *matrix.get(i, j).ok_or(QrError::RenderError {
                reason: format!("Invalid read at i: {i}, j: {j} during svg render."),
            })?;
            if 0 == val {
                // Just cast everything to f32
                let mut rect = Rectangle::new()
                    .set("x", j as f32 * block_size)
                    .set("y", i as f32 * block_size)
                    .set("width", block_size)
                    .set("height", block_size)
                    .set("fill", p_bg_col);

                // Handle additional optional features:
                //
                // Corner Radius (x)
                if let Some(rx) = pixel_hints.rx {
                    rect = rect.set("rx", rx);
                }
                // Corner Radius (y)
                if let Some(ry) = pixel_hints.ry {
                    rect = rect.set("ry", ry);
                }

                // Shape Rendering
                if let Some(shape_rendering) = hints.shape_rendering {
                    rect = rect.set("shape-rendering", shape_rendering);
                }

                if let Some(stroke_hints) = pixel_hints.stroke {
                    if let Some(width) = stroke_hints.width {
                        rect = rect.set("stroke-width", width);
                    }
                    if let Some(color) = stroke_hints.color {
                        rect = rect.set("stroke", color);
                    }
                }

                doc = doc.add(rect);
            }
        }
    }

    Ok(doc)
}

// Supply None to just use the matrix.side_length()
// TODO: implement actual error types and return a proper error
// TODO TWICE: (see above re: resizing issues).
// TODO: expose rounded-corner border radius.
#[cfg(feature = "svg")]
pub fn save_svg(
    file_path: &Path,
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
    hints: Option<SvgHints>,
) -> Result<()> {
    let svg = render_svg_without_resampling(matrix, side_length, hints)?;
    Ok(write_svg(file_path, &svg)?)
}

#[inline]
#[cfg(feature = "svg")]
pub(crate) fn write_svg(file_path: &Path, svg: &Document) -> Result<()> {
    Ok(svg::save(file_path, svg)?)
}

// --------- IMAGES -------

#[inline]
#[cfg(feature = "image")]
pub fn render_image(matrix: &SquareMatrix<u8>) -> Result<DynamicImage> {
    let n = matrix.side_length();
    let mut img = GrayImage::new(n as u32, n as u32);
    // The buffer has to be written, since this the assumption is 8bits per pixel.
    // It thus has an r, g, and b channel.
    for i in 0..n {
        for j in 0..n {
            let val = *matrix.get(i, j).ok_or(QrError::RenderError {
                reason: format!("Invalid read at i: {i}, j: {j} when rendering image."),
            })?;
            // Multiply it by 255, black will cancel this out.
            let color = val * 0xFF;
            *img.get_pixel_mut(j as u32, i as u32) = Luma([color]);
        }
    }

    Ok(DynamicImage::ImageLuma8(img))
}

// TODO: docstring
// This takes a best-effort approach to find the nearest integer k to multiply/divide the old
// side_length by to reach the supplied side_length if it is Some.
// Then it performs a nearest-neighbor resampling before returning a dynamic image for further
// processing.
#[cfg(feature = "image")]
pub fn resize_and_render_image(
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Result<DynamicImage> {
    let old_side_length = matrix.side_length();

    let n = side_length
        .and_then(|new_len| {
            let mul = nearest_integer_multiple(old_side_length, new_len);

            match mul {
                Ok(mul) => {
                    let new_len = match mul {
                        IntegerInverse::Divide(n) => {
                            if n == 0 {
                                return Some(Err(QrError::ArithmeticError(
                                    ArithmeticError::ZeroDivision,
                                )));
                            }
                            old_side_length / n
                        }
                        IntegerInverse::Multiply(n) => old_side_length * n,
                    };
                    Some(Ok(new_len))
                }
                Err(e) => Some(Err(e)),
            }
        })
        .unwrap_or(Ok(old_side_length))?;

    // Destructuring the SquareMatrix consumes it, so this has to clone
    // the borrow.

    // If the size is not the same size as the old side_len, the matrix has to be resized
    let export_image = if n != old_side_length {
        matrix.resize(n)?
    } else {
        matrix.clone()
    };

    let mut img = GrayImage::new(n as u32, n as u32);

    // The buffer has to be written, since this the assumption is 8bits per pixel.
    // It thus has an r, g, and b channel.
    for i in 0..n {
        for j in 0..n {
            let val = *export_image.get(i, j).ok_or(QrError::RenderError {
                reason: format!("Invalid read at i: {i}, j: {j} during image render."),
            })?;
            // Multiply it by 255, black will cancel this out.
            let color = val * 0xFF;
            *img.get_pixel_mut(j as u32, i as u32) = Luma([color]);
        }
    }

    Ok(DynamicImage::ImageLuma8(img))
}

// TODO: refactor this interface:
//  ->  one function that takes in an optional side length for export
//      (this one should use the nearest integer for scaling)
//      -> The nearest neighbor implementation should be identical to the image crate
//         so it doesn't really matter which gets used.
//
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
// Again, as mentioned above, it is ill advised to resize below the minimum size required
// for the given QR version.
#[cfg(feature = "image")]
pub fn resize_and_render_image_exact(
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Result<(DynamicImage, bool)> {
    let old_side_length = matrix.side_length();
    let n = match side_length {
        Some(new_len) => new_len,
        None => old_side_length,
    };

    // Destructuring the SquareMatrix consumes it, so this has to clone
    // the borrow.

    // If the size is not the same size as the old side_len, the matrix has to be resized
    let export_image = if n != old_side_length {
        matrix.resize(n)?
    } else {
        matrix.clone()
    };

    let mut img = GrayImage::new(n as u32, n as u32);

    // The buffer has to be written, since this the assumption is 8bits per pixel.
    // It thus has an r, g, and b channel.
    for i in 0..n {
        for j in 0..n {
            let val = *export_image.get(i, j).ok_or(QrError::RenderError {
                reason: format!("Invalid read at i: {i}, j:{j} during image render."),
            })?;
            // Multiply it by 255, black will cancel this out.
            let color = val * 0xFF;
            *img.get_pixel_mut(j as u32, i as u32) = Luma([color]);
        }
    }

    let is_fract = if n > old_side_length {
        n.rem_euclid(old_side_length) > 0
    } else {
        old_side_length.rem_euclid(n) > 0
    };

    Ok((DynamicImage::ImageLuma8(img), is_fract))
}

// TODO: document -> if users wish to use a different type of format, they can operate on the
// DynamicImage returned from the above rendering functions.
#[inline]
#[cfg(feature = "png")]
pub fn save_png(
    file_path: &Path,
    matrix: &SquareMatrix<u8>,
    side_length: Option<usize>,
) -> Result<()> {
    let png = resize_and_render_image(matrix, side_length)?;
    write_png(file_path, &png)
}

#[inline]
#[cfg(feature = "png")]
pub(crate) fn write_png(file_path: &Path, png: &DynamicImage) -> Result<()> {
    let format = ImageFormat::Png;
    Ok(png.save_with_format(file_path, format)?)
}
