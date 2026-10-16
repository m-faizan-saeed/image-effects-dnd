use std::sync::Arc;

use eframe::egui::Color32;
use image::DynamicImage;

use crate::imageproc_util::draw_watermark;

#[derive(Clone, Debug, PartialEq)]
pub struct WatermarkParams {
    pub text: String,
    pub color: Color32,
    pub x: i32,
    pub y: i32,
    pub scale: f32,
    pub degree: f32,
}

impl Default for WatermarkParams {
    fn default() -> Self {
        Self {
            text: "My Watermark\nMultiline".to_string(),
            color: Color32::from_rgb(0, 0, 0),
            x: -190,
            y: -190,
            scale: 24.0,
            degree: -45.0,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum EffectType {
    Blur { sigma: f32 },
    Sharpen { sigma: f32, threshold: i32 },
    Brightness { value: i32 },
    Contrast { value: f32 },
    MagicColor { whitening: f32, color_boost: f32 },
    Binarize { window_size: u32, bias: i32 },
    Grayscale,
    Lighten { gamma: f32, brightness: i32 },
    InkSaver { threshold: u8, thinning: u8 },
    Watermark { params: WatermarkParams },
}

#[derive(Clone)]
pub(crate) struct ImageOp {
    pub(crate) id: usize,
    pub(crate) enabled: bool,
    pub(crate) effect: EffectType,
}

impl PartialEq for ImageOp {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for ImageOp {}
impl std::hash::Hash for ImageOp {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

pub(crate) struct ImageEditor {
    pub(crate) pipeline: Vec<ImageOp>,
    pub(crate) next_id: usize,
    pub(crate) original_image: Arc<DynamicImage>,
    pub(crate) final_image: Option<DynamicImage>,
}

impl ImageEditor {
    pub(crate) fn new_image_op(&mut self, effect: EffectType) -> ImageOp {
        let img_op = ImageOp {
            id: self.next_id,
            enabled: true,
            effect: effect,
        };
        self.next_id += 1;
        return img_op;
    }

    pub(crate) fn push_new_img_op(&mut self, effect: EffectType) {
        let img_op = self.new_image_op(effect);
        self.pipeline.push(img_op);
    }

    pub(crate) fn new() -> Self {
        // Create a dummy gradient image
        let img = image::ImageBuffer::from_fn(512, 512, |x, y| {
            image::Rgba([(x / 2) as u8, (y / 2) as u8, 128, 255])
        });
        let dynamic_img = DynamicImage::ImageRgba8(img);

        Self {
            pipeline: vec![],
            next_id: 0,
            original_image: Arc::new(dynamic_img),
            final_image: None,
        }
    }

    pub(crate) fn save_image(&self, path: &str) {
        if let Some(img) = &self.final_image {
            img.save(path);
        }
    }

    pub(crate) fn render_image(original: &DynamicImage, pipeline: &[ImageOp]) -> DynamicImage {
        let mut img = original.clone();

        for op in pipeline {
            if !op.enabled {
                continue;
            }
            match &op.effect {
                EffectType::Blur { sigma } if *sigma > 0.0 => img = img.blur(*sigma),
                EffectType::Sharpen { sigma, threshold } => img = img.unsharpen(*sigma, *threshold),
                EffectType::Brightness { value } => img = img.brighten(*value),
                EffectType::Contrast { value } => img = img.adjust_contrast(*value),
                EffectType::MagicColor {
                    whitening,
                    color_boost,
                } => img = magic_color(&img, *whitening, *color_boost),
                EffectType::Binarize { window_size, bias } => {
                    img = binarize(&img, *window_size, *bias)
                }
                EffectType::Grayscale => img = grayscale(&img),
                EffectType::Lighten { gamma, brightness } => {
                    img = lighten(&img, *gamma, *brightness)
                }
                EffectType::InkSaver {
                    threshold,
                    thinning,
                } => img = ink_saver(&img, *threshold, *thinning),
                EffectType::Watermark { params } => {
                    let _ = draw_watermark(&mut img, params);
                }
                EffectType::Blur { .. } => {}
            }
        }

        img
    }
}

fn magic_color(image: &DynamicImage, whitening: f32, color_boost: f32) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    let gray = image.to_luma8();
    let radius = ((image.width().min(image.height()) / 40).clamp(8, 48)) as f32;
    let background = image::imageops::blur(&gray, radius);
    let whitening = whitening.clamp(0.0, 1.0);
    let color_boost = color_boost.clamp(0.0, 2.0);

    for (pixel, local_bg) in rgba.pixels_mut().zip(background.pixels()) {
        let lift = ((255.0 - local_bg[0] as f32) * whitening).round();
        let luminance = 0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32;
        for channel in &mut pixel.0[..3] {
            let chroma = *channel as f32 - luminance;
            *channel = (luminance + lift + chroma * (1.0 + color_boost))
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    DynamicImage::ImageRgba8(rgba)
}

fn binarize(image: &DynamicImage, window_size: u32, bias: i32) -> DynamicImage {
    let rgba = image.to_rgba8();
    let gray = image.to_luma8();
    let (width, height) = gray.dimensions();
    let window_size = window_size.clamp(3, 101) | 1;
    let radius = window_size / 2;
    let bias = bias.clamp(0, 64) as i32;
    let mut column_sums = vec![0_u32; width as usize];
    let mut output = image::RgbaImage::new(width, height);

    for y in 0..height {
        if y == 0 {
            for row in 0..=(radius.min(height - 1)) {
                for x in 0..width {
                    column_sums[x as usize] += gray.get_pixel(x, row)[0] as u32;
                }
            }
        } else {
            let entering_y = y + radius;
            let leaving_y = y.saturating_sub(radius + 1);
            for x in 0..width {
                if entering_y < height {
                    column_sums[x as usize] += gray.get_pixel(x, entering_y)[0] as u32;
                }
                if y > radius && leaving_y < height {
                    column_sums[x as usize] -= gray.get_pixel(x, leaving_y)[0] as u32;
                }
            }
        }

        let mut horizontal_sum = 0_u64;
        for x in 0..width {
            let right = (x + radius).min(width - 1) as usize;
            let left = x.saturating_sub(radius) as usize;
            if x == 0 {
                horizontal_sum = column_sums[..=right]
                    .iter()
                    .map(|&value| value as u64)
                    .sum();
            } else {
                let entering_x = (x + radius).min(width - 1) as usize;
                let previous_right = (x - 1 + radius).min(width - 1) as usize;
                if entering_x > previous_right {
                    horizontal_sum += column_sums[entering_x] as u64;
                }
                let previous_left = (x - 1).saturating_sub(radius) as usize;
                if previous_left < left {
                    horizontal_sum -= column_sums[previous_left] as u64;
                }
            }
            let local_width = (right - left + 1) as u64;
            let top = y.saturating_sub(radius);
            let bottom = (y + radius).min(height - 1);
            let local_height = (bottom - top + 1) as u64;
            let threshold = (horizontal_sum / (local_width * local_height)) as i32 + bias;
            let value = if gray.get_pixel(x, y)[0] as i32 <= threshold {
                0
            } else {
                255
            };
            let source = rgba.get_pixel(x, y);
            output.put_pixel(x, y, image::Rgba([value, value, value, source[3]]));
        }
    }

    DynamicImage::ImageRgba8(output)
}

fn grayscale(image: &DynamicImage) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    for pixel in rgba.pixels_mut() {
        let value = (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32)
            .round() as u8;
        pixel[0] = value;
        pixel[1] = value;
        pixel[2] = value;
    }
    DynamicImage::ImageRgba8(rgba)
}

fn lighten(image: &DynamicImage, gamma: f32, brightness: i32) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    let gamma = gamma.clamp(0.2, 1.0);
    let brightness = brightness.clamp(-64, 96) as f32;
    for pixel in rgba.pixels_mut() {
        for channel in &mut pixel.0[..3] {
            let normalized = *channel as f32 / 255.0;
            *channel = ((normalized.powf(gamma) * 255.0) + brightness)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    DynamicImage::ImageRgba8(rgba)
}

fn ink_saver(image: &DynamicImage, threshold: u8, thinning: u8) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    for pixel in rgba.pixels_mut() {
        let luminance =
            (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32) as u8;
        if luminance >= threshold {
            pixel[0] = 255;
            pixel[1] = 255;
            pixel[2] = 255;
        } else {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
        }
    }

    let (width, height) = rgba.dimensions();
    for _ in 0..thinning.min(2) {
        let source = rgba.clone();
        for y in 1..height.saturating_sub(1) {
            for x in 1..width.saturating_sub(1) {
                if source.get_pixel(x, y)[0] == 0 {
                    let black_neighbors = (y - 1..=y + 1)
                        .flat_map(|ny| (x - 1..=x + 1).map(move |nx| (nx, ny)))
                        .filter(|&(nx, ny)| source.get_pixel(nx, ny)[0] == 0)
                        .count();
                    if black_neighbors >= 7 {
                        let pixel = rgba.get_pixel_mut(x, y);
                        pixel[0] = 255;
                        pixel[1] = 255;
                        pixel[2] = 255;
                    }
                }
            }
        }
    }
    DynamicImage::ImageRgba8(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn grayscale_keeps_alpha_and_removes_color() {
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(1, 1, Rgba([200, 100, 50, 127])));
        let result = grayscale(&image).to_rgba8().get_pixel(0, 0).0;
        assert_eq!(result[0], result[1]);
        assert_eq!(result[1], result[2]);
        assert_eq!(result[3], 127);
    }

    #[test]
    fn adaptive_binarization_produces_only_black_and_white() {
        let mut pixels = ImageBuffer::from_pixel(9, 9, Rgba([240, 240, 240, 255]));
        pixels.put_pixel(4, 4, Rgba([20, 20, 20, 255]));
        let image = DynamicImage::ImageRgba8(pixels);

        let result = binarize(&image, 5, 0).to_rgba8();
        assert_eq!(result.get_pixel(4, 4)[0], 0);
        assert!(
            result
                .pixels()
                .all(|pixel| pixel[0] == 0 || pixel[0] == 255)
        );
    }

    #[test]
    fn ink_saver_whitens_background_and_preserves_alpha() {
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 1, Rgba([230, 230, 230, 100])));
        let result = ink_saver(&image, 220, 0).to_rgba8();
        assert_eq!(result.get_pixel(0, 0).0, [255, 255, 255, 100]);
    }
}
