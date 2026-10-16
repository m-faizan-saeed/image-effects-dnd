use eframe::egui;
use egui_dnd::dnd;
use rfd::FileDialog;
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
};

use crate::image_editor::{EffectType, ImageEditor, ImageOp, WatermarkParams};

pub(crate) struct ImageEditorUi {
    img_editor: ImageEditor,
    display_texture: Option<egui::TextureHandle>,
    dirty: bool,
    zoom_percent: f32,
    source_path: Option<PathBuf>,
    render_sender: mpsc::Sender<RenderRequest>,
    render_receiver: mpsc::Receiver<RenderResult>,
    render_revision: u64,
    rendering: bool,
    error_message: Option<String>,
}

struct RenderRequest {
    revision: u64,
    source: RenderSource,
    pipeline: Vec<ImageOp>,
    max_texture_side: u32,
}

enum RenderSource {
    Existing(Arc<image::DynamicImage>),
    Path(PathBuf),
}

struct RenderResult {
    revision: u64,
    image: Result<
        (
            Arc<image::DynamicImage>,
            image::DynamicImage,
            image::RgbaImage,
        ),
        String,
    >,
}

impl ImageEditorUi {
    pub(crate) fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (render_sender, request_receiver) = mpsc::channel::<RenderRequest>();
        let (result_sender, render_receiver) = mpsc::channel::<RenderResult>();
        let context = _cc.egui_ctx.clone();

        thread::spawn(move || {
            while let Ok(mut request) = request_receiver.recv() {
                // while let Ok(newer_request) = request_receiver.try_recv() {
                //     request = newer_request;
                // }

                let image = match request.source {
                    RenderSource::Existing(image) => Ok(image),
                    RenderSource::Path(path) => image::ImageReader::open(path)
                        .map_err(|error| error.to_string())
                        .and_then(|reader| reader.decode().map_err(|error| error.to_string()))
                        .map(Arc::new),
                }
                .map(|original| {
                    let final_image = ImageEditor::render_image(&original, &request.pipeline);
                    let preview = final_image
                        .thumbnail(request.max_texture_side, request.max_texture_side)
                        .to_rgba8();
                    (original, final_image, preview)
                });

                if result_sender
                    .send(RenderResult {
                        revision: request.revision,
                        image,
                    })
                    .is_err()
                {
                    break;
                }
                context.request_repaint();
            }
        });

        Self {
            img_editor: ImageEditor::new(),
            display_texture: None,
            dirty: true,
            zoom_percent: 100.0,
            source_path: None,
            render_sender,
            render_receiver,
            render_revision: 0,
            rendering: false,
            error_message: None,
        }
    }

    fn request_render(&mut self, ctx: &egui::Context) {
        self.render_revision += 1;
        let max_texture_side = ctx
            .input(|input| input.max_texture_side)
            .min(u32::MAX as usize)
            .max(1) as u32;
        let request = RenderRequest {
            revision: self.render_revision,
            source: self.source_path.clone().map_or_else(
                || RenderSource::Existing(self.img_editor.original_image.clone()),
                RenderSource::Path,
            ),
            pipeline: self.img_editor.pipeline.clone(),
            max_texture_side,
        };
        self.rendering = true;
        self.error_message = None;
        if self.render_sender.send(request).is_err() {
            self.error_message = Some("Image processing worker is unavailable.".to_string());
            self.rendering = false;
        }
        self.dirty = false;
    }

    fn receive_rendered_image(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.render_receiver.try_recv() {
            if result.revision != self.render_revision {
                continue;
            }
            self.rendering = false;
            match result.image {
                Ok((original, final_image, preview)) => {
                    self.source_path = None;
                    self.img_editor.original_image = original;
                    self.img_editor.final_image = Some(final_image);
                    self.display_texture = Some(ctx.load_texture(
                        "display",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [preview.width() as usize, preview.height() as usize],
                            preview.as_raw(),
                        ),
                        Default::default(),
                    ));
                    self.error_message = None;
                }
                Err(error) => {
                    self.source_path = None;
                    self.error_message = Some(format!("Could not open image: {error}"));
                }
            }
        }
    }
}

impl eframe::App for ImageEditorUi {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_rendered_image(ctx);
        // egui::Window::new("Floating Tool")
        //     .default_pos([100.0, 100.0])
        //     .show(ctx, |ui| {
        //         ui.label("I am a floating window");
        //     });

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        if let Some(path) = FileDialog::new()
                            .add_filter(
                                "Image Files",
                                &["png", "jpg", "jpeg", "gif", "bmp", "tiff", "webp"],
                            )
                            .pick_file()
                        {
                            self.source_path = Some(path);
                            self.dirty = true;
                        }
                    }
                    if ui.button("Save").clicked() {
                        self.img_editor.save_image("out.jpg");
                    }
                });
            });
        });

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
                self.request_render(ctx);
            }
            if self.rendering {
                ui.spinner();
                ui.label("Processing image...");
            }
            if let Some(error) = &self.error_message {
                ui.colored_label(egui::Color32::RED, error);
            }
            ui.horizontal(|ui| {
                if ui.button("-").clicked() {
                    self.zoom_percent = (self.zoom_percent - 10.0).max(10.0);
                }
                ui.add(
                    egui::Slider::new(&mut self.zoom_percent, 10.0..=400.0)
                        .text("Zoom")
                        .suffix("%"),
                );
                if ui.button("+").clicked() {
                    self.zoom_percent = (self.zoom_percent + 10.0).min(400.0);
                }
            });

            if let Some(texture) = &self.display_texture {
                let available_width = ui.available_width();
                let image_width = self.img_editor.original_image.width() as f32;
                let fit_width = available_width.min(image_width).max(1.0);
                let display_width = fit_width * self.zoom_percent / 100.0;
                let aspect_ratio = texture.size_vec2().y / texture.size_vec2().x;

                egui::ScrollArea::both().show(ui, |ui| {
                    ui.image((
                        texture.id(),
                        egui::vec2(display_width, display_width * aspect_ratio),
                    ));
                });
            }
        });
    }
}
