use eframe::egui;
use egui_dnd::dnd;
use image::DynamicImage;

use crate::image_editor::{EffectType, ImageEditor, WatermarkParams};

pub(crate) struct ImageEditorUi {
    img_editor: ImageEditor,
    display_texture: Option<egui::TextureHandle>,
    dirty: bool,
}

impl ImageEditorUi {
    pub(crate) fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            img_editor: ImageEditor::new(),
            display_texture: None,
            dirty: true,
        }
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        self.img_editor.process_image();
        if let Some(img) = &self.img_editor.final_image {
            self.display_texture = Some(ctx.load_texture(
                "display",
                egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_bytes(),
                ),
                Default::default(),
            ));
        }

        self.dirty = false;
    }
}

impl eframe::App for ImageEditorUi {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // egui::Window::new("Floating Tool")
        //     .default_pos([100.0, 100.0])
        //     .show(ctx, |ui| {
        //         ui.label("I am a floating window");
        //     });

        egui::SidePanel::left("layers_panel").show(ctx, |ui| {
            ui.heading("Modifier Stack");
            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("+ Blur").clicked() {
                    self.img_editor
                        .push_new_img_op(EffectType::Blur { sigma: 2.0 });
                    self.dirty = true;
                }
                if ui.button("+ Bright").clicked() {
                    self.img_editor
                        .push_new_img_op(EffectType::Brightness { value: 10 });
                    self.dirty = true;
                }
                if ui.button("+ Contrast").clicked() {
                    self.img_editor
                        .push_new_img_op(EffectType::Contrast { value: 1.2 });
                    self.dirty = true;
                }
                if ui.button("+ Text").clicked() {
                    self.img_editor.push_new_img_op(EffectType::Watermark {
                        params: WatermarkParams::default(),
                    });
                    self.dirty = true;
                }
            });

            ui.separator();

            let half_width = (self.img_editor.original_image.width() / 2) as i32;
            let half_height = (self.img_editor.original_image.height() / 2) as i32;

            let mut remove_index: Option<usize> = None;

            let response = dnd(ui, "effect_dnd").show_vec(
                &mut self.img_editor.pipeline,
                |ui, item, handle, state| {
                    ui.horizontal(|ui| {
                        handle.ui(ui, |ui| {
                            ui.label("::");
                        });
                        // if ui.button("❌").clicked() {
                        //     // state.index gives us the current position of this item in the vector
                        //     remove_index = Some(state.index);
                        // }

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
                                // if ui.button("-").clicked() {
                                //     remove_index = Some(item.id);
                                // }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Color");
                                        if ui.color_edit_button_srgba(&mut params.color).changed() {
                                            self.dirty = true;
                                        }
                                        ui.label("Scale");
                                        if ui
                                            .add(egui::Slider::new(&mut params.scale, 1.0..=100.0))
                                            .changed()
                                        {
                                            self.dirty = true;
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        // ui.label("Text");
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
                                                -half_width..=half_width,
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
                                                -half_height..=half_height as i32,
                                            ))
                                            .changed()
                                        {
                                            self.dirty = true;
                                        }
                                    });
                                });
                            }
                        }

                        // ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        //     if ui.button("❌").clicked() {
                        //         // state.index gives us the current position of this item in the vector
                        //         remove_index = Some(state.index);
                        //     }
                        // });
                    });
                },
            );

            if let Some(idx) = remove_index {
                self.img_editor.pipeline.remove(idx);
                self.dirty = true; // Tell the app to re-process the image
            }

            if response.final_update().is_some() {
                self.dirty = true;
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.dirty {
                self.update_texture(ctx);
            }
            if let Some(texture) = &self.display_texture {
                ui.image((texture.id(), texture.size_vec2()));
            }
        });
    }
}
