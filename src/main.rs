use std::f32::consts::PI;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use eframe::egui;
use egui_dnd::dnd;
use image::{DynamicImage, Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Linear Image Editor",
        options,
        Box::new(|cc| Ok(Box::new(MyApp::new(cc)))),
    )
}

#[derive(Clone, Debug, PartialEq)] // Derive traits here
pub struct WatermarkParams {
    pub text: String,
    pub color: egui::Color32,
    pub x: i32,
    pub y: i32,
    pub scale: f32,
    pub degree: f32,
}

// You can even impl Default to make initialization cleaner
impl Default for WatermarkParams {
    fn default() -> Self {
        Self {
            text: "My Watermark".to_string(),
            color: egui::Color32::from_rgb(0, 0, 0),
            x: 180,
            y: 0,
            scale: 36.0,
            degree: -45.0,
        }
    }
}

#[derive(Clone, Debug)]
enum EffectType {
    Blur { sigma: f32 },
    Brightness { value: i32 },
    Contrast { value: f32 },
    Watermark { params: WatermarkParams },
}

#[derive(Clone)]
struct ImageOp {
    id: usize,
    effect: EffectType,
}

// FIX 2: Manual Hash/Eq Implementation to handle floats
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
// END FIX 2

pub fn draw_multiline_text(
    image: &mut RgbaImage,
    color: Rgba<u8>,
    x: i32,
    mut y: i32,
    scale: PxScale,
    font: &FontVec,
    text: &str,
) {
    // 1. Determine how tall each line should be.
    // 'v_metrics' gives us the ascent, descent, and gap of the font.
    let v_metrics = font.as_scaled(scale);

    // Calculate line height: (ascent - descent) + gap
    // We add a small buffer (e.g., + 2.0) for cleaner separation if needed.
    let line_height =
        (v_metrics.ascent() - v_metrics.descent() + v_metrics.line_gap()).ceil() as i32;

    // 2. Loop through each line in the string
    for line in text.lines() {
        if !line.is_empty() {
            draw_text_mut(image, color, x, y, scale, font, line);
        }

        // 3. Move the cursor down for the next line
        y += line_height;
    }
}

fn image_watermark(
    image: &mut DynamicImage,
    params: &WatermarkParams,
) -> Result<(), Box<dyn std::error::Error>> {
    let scale = PxScale::from(24.0);
    // let font_vec = Vec::from(include_bytes!("../DejaVuSans.ttf") as &[u8]);
    let font_vec = Vec::from(include_bytes!("../Roboto-VariableFont_wdth,wght.ttf") as &[u8]);
    let font = FontVec::try_from_vec(font_vec)?;
    let mut text_image: image::ImageBuffer<Rgba<u8>, Vec<u8>> =
        image::ImageBuffer::new(image.width(), image.height());
    // draw_text_mut(
    draw_multiline_text(
        &mut text_image,
        Rgba(params.color.to_array()),
        (image.width() / 2).try_into()?,
        // params.x as i32,
        (image.height() / 2).try_into()?,
        // params.y as i32,
        scale,
        &font,
        &params.text,
    );
    let theta = params.degree * (PI / 180.0);
    text_image = imageproc::geometric_transformations::rotate_about_center(
        &text_image,
        theta,
        imageproc::geometric_transformations::Interpolation::Bicubic,
        Rgba([0, 0, 0, 0]),
    );
    image::imageops::overlay(image, &text_image, 0, 0);
    image::imageops::overlay(image, &text_image, params.x as i64, params.y as i64);
    Ok(())
}

struct MyApp {
    pipeline: Vec<ImageOp>,
    next_id: usize,
    original_image: DynamicImage,
    display_texture: Option<egui::TextureHandle>,
    dirty: bool,
}

impl MyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Create a dummy gradient image
        let img = image::ImageBuffer::from_fn(512, 512, |x, y| {
            image::Rgba([(x / 2) as u8, (y / 2) as u8, 128, 255])
        });
        let dynamic_img = DynamicImage::ImageRgba8(img);

        let texture = cc.egui_ctx.load_texture(
            "display",
            egui::ColorImage::from_rgba_unmultiplied(
                [dynamic_img.width() as usize, dynamic_img.height() as usize],
                dynamic_img.as_bytes(),
            ),
            Default::default(),
        );

        Self {
            pipeline: vec![],
            next_id: 0,
            original_image: dynamic_img,
            display_texture: Some(texture),
            dirty: false,
        }
    }

    fn process_image(&mut self, ctx: &egui::Context) {
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
                    let _ = image_watermark(&mut img, params);
                }
            }
        }

        self.display_texture = Some(ctx.load_texture(
            "display",
            egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_bytes(),
            ),
            Default::default(),
        ));

        self.dirty = false;
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("layers_panel").show(ctx, |ui| {
            ui.heading("Modifier Stack");
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("+ Blur").clicked() {
                    self.pipeline.push(ImageOp {
                        id: self.next_id,
                        effect: EffectType::Blur { sigma: 2.0 },
                    });
                    self.next_id += 1;
                    self.dirty = true;
                }
                if ui.button("+ Bright").clicked() {
                    self.pipeline.push(ImageOp {
                        id: self.next_id,
                        effect: EffectType::Brightness { value: 10 },
                    });
                    self.next_id += 1;
                    self.dirty = true;
                }
                if ui.button("+ Contrast").clicked() {
                    self.pipeline.push(ImageOp {
                        id: self.next_id,
                        effect: EffectType::Contrast { value: 1.2 },
                    });
                    self.next_id += 1;
                    self.dirty = true;
                }
                if ui.button("+ Text").clicked() {
                    self.pipeline.push(ImageOp {
                        id: self.next_id,
                        effect: EffectType::Watermark {
                            params: WatermarkParams::default(),
                        },
                    });
                    self.next_id += 1;
                    self.dirty = true;
                }
            });

            ui.separator();

            let response =
                dnd(ui, "effect_dnd").show_vec(&mut self.pipeline, |ui, item, handle, _state| {
                    ui.horizontal(|ui| {
                        handle.ui(ui, |ui| {
                            ui.label("::");
                        });

                        match &mut item.effect {
                            EffectType::Blur { sigma } => {
                                ui.label("Blur");
                                if ui.add(egui::Slider::new(sigma, 0.0..=10.0)).changed() {
                                    self.dirty = true;
                                }
                            }
                            EffectType::Brightness { value } => {
                                ui.label("Bright");
                                if ui.add(egui::Slider::new(value, -100..=100)).changed() {
                                    self.dirty = true;
                                }
                            }
                            EffectType::Contrast { value } => {
                                ui.label("Contrast");
                                if ui.add(egui::Slider::new(value, 0.0..=5.0)).changed() {
                                    self.dirty = true;
                                }
                            }
                            EffectType::Watermark { params } => {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Text");
                                        if ui
                                            .add(egui::TextEdit::multiline(&mut params.text))
                                            .changed()
                                        {
                                            self.dirty = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Angle");
                                        if ui
                                            .add(egui::Slider::new(
                                                &mut params.degree,
                                                -180.0..=180.0,
                                            ))
                                            .changed()
                                        {
                                            self.dirty = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("X");
                                        if ui
                                            .add(egui::Slider::new(
                                                &mut params.x,
                                                0..=self.original_image.width() as i32,
                                            ))
                                            .changed()
                                        {
                                            self.dirty = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Y");
                                        if ui
                                            .add(egui::Slider::new(
                                                &mut params.y,
                                                0..=self.original_image.height() as i32,
                                            ))
                                            .changed()
                                        {
                                            self.dirty = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Color");
                                        if ui.color_edit_button_srgba(&mut params.color).changed() {
                                            self.dirty = true;
                                        }
                                    });
                                });
                            }
                        }
                    });
                });

            if response.final_update().is_some() {
                self.dirty = true;
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.dirty {
                self.process_image(ctx);
            }
            if let Some(texture) = &self.display_texture {
                ui.image((texture.id(), texture.size_vec2()));
            }
        });
    }
}
