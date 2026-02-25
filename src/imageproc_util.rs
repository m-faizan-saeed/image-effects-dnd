use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use image::{DynamicImage, GenericImage, GenericImageView, Pixel, Rgba, RgbaImage};
use imageproc::{
    drawing::{draw_filled_rect_mut, draw_text_mut},
    rect::Rect,
};
use std::f32::consts::PI;

use crate::{font_util::calculate_line_width, image_editor::WatermarkParams};

pub fn draw_multiline_text_mut(
    image: &mut RgbaImage,
    color: Rgba<u8>,
    x: i32,
    y: i32,
    scale: PxScale,
    font: &FontVec,
    text: &str,
) -> (f32, f32) {
    let scaled_font = font.as_scaled(scale);
    let line_height = scaled_font.height();
    let mut width = 0f32;
    let mut height = 0f32;

    for line in text.lines() {
        if !line.is_empty() {
            let w = calculate_line_width(scaled_font, line);
            width = width.max(w);
            draw_text_mut(
                image,
                color,
                x - ((w as i32) / 2),
                y + height as i32,
                scale,
                font,
                line,
            );
        }
        height += line_height;
    }
    (width, height)
}

pub(crate) fn draw_watermark(
    image: &mut DynamicImage,
    params: &WatermarkParams,
) -> Result<(), Box<dyn std::error::Error>> {
    let scale = PxScale::from(params.scale);
    // let font_vec = Vec::from(include_bytes!("../DejaVuSans.ttf") as &[u8]);
    let font_vec = Vec::from(include_bytes!("../Roboto-VariableFont_wdth,wght.ttf") as &[u8]);
    let font = FontVec::try_from_vec(font_vec)?;
    let mut text_image: image::ImageBuffer<Rgba<u8>, Vec<u8>> =
        image::ImageBuffer::new(image.width(), image.height());

    let color = Rgba(params.color.to_array());
    let x = (image.width() / 2) as i32;
    let y = (image.height() / 2) as i32;

    draw_filled_rect_mut(
        &mut text_image,
        Rect::at(0, y).of_size(image.width(), 2),
        color,
    );
    let (_w, h) = draw_multiline_text_mut(&mut text_image, color, x, y, scale, &font, &params.text);

    draw_filled_rect_mut(
        &mut text_image,
        Rect::at(0, y + h as i32).of_size(image.width(), 2),
        color,
    );
    let theta = params.degree * (PI / 180.0);
    text_image = imageproc::geometric_transformations::rotate_about_center(
        &text_image,
        theta,
        imageproc::geometric_transformations::Interpolation::Bicubic,
        Rgba([0, 0, 0, 0]),
    );
    // image::imageops::overlay(image, &text_image, 0, -100);
    image::imageops::overlay(image, &text_image, params.x as i64, params.y as i64);
    // blend_exclusion2(image, &text_image, params.x as i64, params.y as i64);

    Ok(())
}

fn _blend_difference(
    background: &mut image::DynamicImage,
    text_image: &image::DynamicImage,
    offset_x: i64,
    offset_y: i64,
) {
    let (text_w, text_h) = text_image.dimensions();
    let (bg_w, bg_h) = background.dimensions();

    for y in 0..text_h {
        for x in 0..text_w {
            let bg_x = offset_x + x as i64;
            let bg_y = offset_y + y as i64;

            // 1. Bounds check: Ensure we are inside the background image
            if bg_x >= 0 && bg_x < bg_w as i64 && bg_y >= 0 && bg_y < bg_h as i64 {
                let text_pixel = text_image.get_pixel(x, y);
                let text_rgba = text_pixel.to_rgba();

                // Skip completely transparent pixels to save processing
                if text_rgba[3] == 0 {
                    continue;
                }

                let bg_pixel = background.get_pixel(bg_x as u32, bg_y as u32);
                let bg_rgba = bg_pixel.to_rgba();

                // 2. Calculate Difference for each channel: |Background - Text|
                // Use i16 to prevent underflow during subtraction
                let r_diff = (bg_rgba[0] as i16 - text_rgba[0] as i16).abs() as u8;
                let g_diff = (bg_rgba[1] as i16 - text_rgba[1] as i16).abs() as u8;
                let b_diff = (bg_rgba[2] as i16 - text_rgba[2] as i16).abs() as u8;

                // 3. Alpha Compositing
                // If the text is semi-transparent (anti-aliased), we blend
                // between the original background and the differenced result.
                let alpha_weight = text_rgba[3] as f32 / 255.0;

                let final_r =
                    (bg_rgba[0] as f32 * (1.0 - alpha_weight) + r_diff as f32 * alpha_weight) as u8;
                let final_g =
                    (bg_rgba[1] as f32 * (1.0 - alpha_weight) + g_diff as f32 * alpha_weight) as u8;
                let final_b =
                    (bg_rgba[2] as f32 * (1.0 - alpha_weight) + b_diff as f32 * alpha_weight) as u8;

                // 4. Update the background pixel
                background.put_pixel(
                    bg_x as u32,
                    bg_y as u32,
                    Rgba([final_r, final_g, final_b, bg_rgba[3]]),
                );
            }
        }
    }
}

fn _blend_exclusion2<I, J>(background: &mut I, text_image: &J, offset_x: i64, offset_y: i64)
where
    I: GenericImage,
    J: GenericImageView<Pixel = I::Pixel>,
    I::Pixel: Pixel<Subpixel = u8>,
{
    let (text_w, text_h) = text_image.dimensions();
    let (bg_w, bg_h) = background.dimensions();

    for y in 0..text_h {
        for x in 0..text_w {
            let bg_x = offset_x + x as i64;
            let bg_y = offset_y + y as i64;

            if bg_x >= 0 && bg_x < bg_w as i64 && bg_y >= 0 && bg_y < bg_h as i64 {
                let text_pixel = text_image.get_pixel(x, y);
                let text_rgba = text_pixel.to_rgba();

                // Skip if fully transparent
                if text_rgba[3] == 0 {
                    continue;
                }

                let bg_pixel = background.get_pixel(bg_x as u32, bg_y as u32);
                let bg_rgba = bg_pixel.to_rgba();

                // Exclusion blend logic
                let blend = |bg: u8, fg: u8| -> u8 {
                    let bgf = bg as f32;
                    let fgf = fg as f32;
                    let res = bgf + fgf - (2.0 * bgf * fgf / 255.0);
                    res.clamp(0.0, 255.0) as u8
                };

                let r_excl = blend(bg_rgba[0], text_rgba[0]);
                let g_excl = blend(bg_rgba[1], text_rgba[1]);
                let b_excl = blend(bg_rgba[2], text_rgba[2]);

                // Alpha blending (handles anti-aliased text edges)
                let alpha = text_rgba[3] as f32 / 255.0;
                let final_r = (bg_rgba[0] as f32 * (1.0 - alpha) + r_excl as f32 * alpha) as u8;
                let final_g = (bg_rgba[1] as f32 * (1.0 - alpha) + g_excl as f32 * alpha) as u8;
                let final_b = (bg_rgba[2] as f32 * (1.0 - alpha) + b_excl as f32 * alpha) as u8;

                // Prepare the raw byte array [R, G, B, A]
                // We keep the background's original alpha (bg_rgba[3])
                let raw_pixel = [final_r, final_g, final_b, bg_rgba[3]];

                // Use from_slice to create the pixel type generic over I::Pixel
                let final_pixel = *I::Pixel::from_slice(&raw_pixel);

                background.put_pixel(bg_x as u32, bg_y as u32, final_pixel);
            }
        }
    }
}
