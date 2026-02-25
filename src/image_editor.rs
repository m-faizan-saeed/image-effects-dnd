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
    Brightness { value: i32 },
    Contrast { value: f32 },
    Watermark { params: WatermarkParams },
}

#[derive(Clone)]
pub(crate) struct ImageOp {
    pub(crate) id: usize,
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
    pub(crate) original_image: DynamicImage,
    pub(crate) final_image: Option<DynamicImage>,
}

impl ImageEditor {
    pub(crate) fn new_image_op(&mut self, effect: EffectType) -> ImageOp {
        let img_op = ImageOp {
            id: self.next_id,
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
            original_image: dynamic_img,
            final_image: None,
        }
    }

    pub(crate) fn process_image(&mut self) {
        let mut img = self.original_image.clone();

        for op in &self.pipeline {
            match &op.effect {
                EffectType::Blur { sigma } => {
                    // Check for 0.0 to prevent crash on some blur implementations
                    if *sigma > 0.0 {
                        img = img.blur(*sigma);
                    }
                }
                EffectType::Brightness { value } => {
                    img = img.brighten(*value);
                }
                EffectType::Contrast { value } => {
                    img = img.adjust_contrast(*value);
                }
                EffectType::Watermark { params } => {
                    let _ = draw_watermark(&mut img, params);
                }
            }
        }

        self.final_image = Some(img);
    }
}
