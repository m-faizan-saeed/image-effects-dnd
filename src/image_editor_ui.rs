use eframe::egui;
use egui_dnd::dnd;
use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
    thread,
};

use crate::image_editor::{EffectType, ImageEditor, ImageOp, WatermarkParams};

const PREVIEW_MAX_SIDE: u32 = 1280;
const TEMPLATE_FILE: &str = "modifier-template.toml";

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
    template_message: Option<String>,
    templates: Vec<ModifierTemplate>,
    selected_template: Option<usize>,
    template_name: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct ModifierTemplate {
    name: String,
    modifiers: Vec<TemplateEffect>,
}

#[derive(Serialize, Deserialize)]
struct TemplateCatalog {
    #[serde(default = "template_format_version")]
    version: u32,
    templates: Vec<ModifierTemplate>,
}

#[derive(Deserialize)]
struct LegacyTemplate {
    modifiers: Vec<LegacyTemplateEffect>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LegacyTemplateEffect {
    Blur {
        sigma: f32,
    },
    Brightness {
        value: i32,
    },
    Contrast {
        value: f32,
    },
    Watermark {
        text: String,
        color: [u8; 4],
        x: i32,
        y: i32,
        scale: f32,
        degree: f32,
    },
}

impl Default for TemplateCatalog {
    fn default() -> Self {
        Self {
            version: template_format_version(),
            templates: Vec::new(),
        }
    }
}

fn template_format_version() -> u32 {
    1
}

fn upsert_template(templates: &mut Vec<ModifierTemplate>, template: ModifierTemplate) -> bool {
    let matching_indices = templates
        .iter()
        .enumerate()
        .filter_map(|(index, existing)| {
            existing
                .name
                .eq_ignore_ascii_case(&template.name)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let was_update = !matching_indices.is_empty();
    if let Some(&index) = matching_indices.first() {
        templates[index] = template;
        for &duplicate_index in matching_indices.iter().skip(1).rev() {
            templates.remove(duplicate_index);
        }
    } else {
        templates.push(template);
    }
    was_update
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TemplateEffect {
    Blur {
        sigma: f32,
    },
    Brightness {
        value: i32,
    },
    Contrast {
        value: f32,
    },
    Watermark {
        text: String,
        color: String,
        x: i32,
        y: i32,
        scale: f32,
        degree: f32,
    },
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

fn parse_template_color(value: &str) -> Result<egui::Color32, String> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 8 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "Invalid watermark color '{value}': expected #RRGGBBAA."
        ));
    }
    let channel = |range| {
        u8::from_str_radix(&hex[range], 16)
            .map_err(|_| format!("Invalid watermark color '{value}': expected #RRGGBBAA."))
    };
    Ok(egui::Color32::from_rgba_unmultiplied(
        channel(0..2)?,
        channel(2..4)?,
        channel(4..6)?,
        channel(6..8)?,
    ))
}

impl ImageEditorUi {
    pub(crate) fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (render_sender, request_receiver) = mpsc::channel::<RenderRequest>();
        let (result_sender, render_receiver) = mpsc::channel::<RenderResult>();
        let context = _cc.egui_ctx.clone();

        thread::spawn(move || {
            while let Ok(mut request) = request_receiver.recv() {
                while let Ok(newer_request) = request_receiver.try_recv() {
                    request = newer_request;
                }

                let image = match request.source {
                    RenderSource::Existing(image) => Ok(image),
                    RenderSource::Path(path) => image::ImageReader::open(path)
                        .map_err(|error| error.to_string())
                        .and_then(|reader| reader.decode().map_err(|error| error.to_string()))
                        .map(Arc::new),
                }
                .map(|original| {
                    let final_image = ImageEditor::render_image(&original, &request.pipeline);
                    let preview_side = request.max_texture_side.min(PREVIEW_MAX_SIDE);
                    let preview = final_image.thumbnail(preview_side, preview_side).to_rgba8();
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

        let (templates, template_message) = match Self::read_template_catalog() {
            Ok(catalog) => (catalog.templates, None),
            Err(error) => (Vec::new(), Some(error)),
        };

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
            template_message,
            templates,
            selected_template: None,
            template_name: String::new(),
        }
    }

    fn read_template_catalog() -> Result<TemplateCatalog, String> {
        let contents = match std::fs::read_to_string(TEMPLATE_FILE) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(TemplateCatalog::default());
            }
            Err(error) => return Err(format!("Could not read {TEMPLATE_FILE}: {error}")),
        };
        let value: toml::Value = toml::from_str(&contents)
            .map_err(|error| format!("Could not read {TEMPLATE_FILE}: {error}"))?;
        let catalog = if value.get("templates").is_some() {
            value
                .try_into::<TemplateCatalog>()
                .map_err(|error| format!("Could not read {TEMPLATE_FILE}: {error}"))?
        } else {
            let legacy = value
                .try_into::<LegacyTemplate>()
                .map_err(|error| format!("Could not read {TEMPLATE_FILE}: {error}"))?;
            TemplateCatalog {
                version: template_format_version(),
                templates: vec![ModifierTemplate {
                    name: "Imported template".to_string(),
                    modifiers: legacy
                        .modifiers
                        .into_iter()
                        .map(|effect| match effect {
                            LegacyTemplateEffect::Blur { sigma } => TemplateEffect::Blur { sigma },
                            LegacyTemplateEffect::Brightness { value } => {
                                TemplateEffect::Brightness { value }
                            }
                            LegacyTemplateEffect::Contrast { value } => {
                                TemplateEffect::Contrast { value }
                            }
                            LegacyTemplateEffect::Watermark {
                                text,
                                color,
                                x,
                                y,
                                scale,
                                degree,
                            } => {
                                let [red, green, blue, alpha] =
                                    egui::Color32::from_rgba_premultiplied(
                                        color[0], color[1], color[2], color[3],
                                    )
                                    .to_srgba_unmultiplied();
                                TemplateEffect::Watermark {
                                    text,
                                    color: format!("#{red:02X}{green:02X}{blue:02X}{alpha:02X}"),
                                    x,
                                    y,
                                    scale,
                                    degree,
                                }
                            }
                        })
                        .collect(),
                }],
            }
        };
        if catalog.version != template_format_version() {
            return Err(format!(
                "Unsupported {TEMPLATE_FILE} version {}.",
                catalog.version
            ));
        }
        Ok(catalog)
    }

    fn save_template(&mut self) {
        let name = self.template_name.trim();
        if name.is_empty() {
            self.template_message = Some("Enter a template name.".to_string());
            return;
        }

        let template = ModifierTemplate {
            name: name.to_string(),
            modifiers: self
                .img_editor
                .pipeline
                .iter()
                .map(|op| match &op.effect {
                    EffectType::Blur { sigma } => TemplateEffect::Blur { sigma: *sigma },
                    EffectType::Brightness { value } => {
                        TemplateEffect::Brightness { value: *value }
                    }
                    EffectType::Contrast { value } => TemplateEffect::Contrast { value: *value },
                    EffectType::Watermark { params } => {
                        let [red, green, blue, alpha] = params.color.to_srgba_unmultiplied();
                        TemplateEffect::Watermark {
                            text: params.text.clone(),
                            color: format!("#{red:02X}{green:02X}{blue:02X}{alpha:02X}"),
                            x: params.x,
                            y: params.y,
                            scale: params.scale,
                            degree: params.degree,
                        }
                    }
                })
                .collect(),
        };

        match Self::read_template_catalog().and_then(|mut catalog| {
            let was_update = upsert_template(&mut catalog.templates, template.clone());
            let contents = toml::to_string_pretty(&catalog).map_err(|error| error.to_string())?;
            std::fs::write(TEMPLATE_FILE, contents).map_err(|error| error.to_string())?;
            Ok((catalog.templates, was_update))
        }) {
            Ok((templates, was_update)) => {
                self.templates = templates;
                self.selected_template = self
                    .templates
                    .iter()
                    .position(|existing| existing.name.eq_ignore_ascii_case(name));
                self.template_message = Some(if was_update {
                    "Template updated.".to_string()
                } else {
                    "Template saved.".to_string()
                });
            }
            Err(error) => self.template_message = Some(format!("Could not save template: {error}")),
        }
    }

    fn apply_template(&mut self, index: usize) {
        let Some(template) = self.templates.get(index).cloned() else {
            return;
        };

        let effects = template
            .modifiers
            .into_iter()
            .map(|effect| match effect {
                TemplateEffect::Blur { sigma } => Ok(EffectType::Blur { sigma }),
                TemplateEffect::Brightness { value } => Ok(EffectType::Brightness { value }),
                TemplateEffect::Contrast { value } => Ok(EffectType::Contrast { value }),
                TemplateEffect::Watermark {
                    text,
                    color,
                    x,
                    y,
                    scale,
                    degree,
                } => Ok(EffectType::Watermark {
                    params: WatermarkParams {
                        text,
                        color: parse_template_color(&color)?,
                        x,
                        y,
                        scale,
                        degree,
                    },
                }),
            })
            .collect::<Result<Vec<_>, String>>();

        match effects {
            Ok(effects) => {
                self.img_editor.pipeline.clear();
                for effect in effects {
                    self.img_editor.push_new_img_op(effect);
                }
                self.dirty = true;
                self.template_message = None;
            }
            Err(error) => self.template_message = Some(error),
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
            ui.horizontal(|ui| {
                let selected_name = self
                    .selected_template
                    .and_then(|index| self.templates.get(index))
                    .map_or("Choose template...", |template| template.name.as_str());
                let mut selected = self.selected_template;
                egui::ComboBox::from_id_salt("modifier_template")
                    .selected_text(selected_name)
                    .show_ui(ui, |ui| {
                        for (index, template) in self.templates.iter().enumerate() {
                            ui.selectable_value(&mut selected, Some(index), &template.name);
                        }
                    });
                if selected != self.selected_template {
                    self.selected_template = selected;
                    if let Some(index) = selected {
                        if let Some(template) = self.templates.get(index) {
                            self.template_name = template.name.clone();
                        }
                        self.apply_template(index);
                    }
                }
                let trimmed_name = self.template_name.trim();
                let overwrites_existing = !trimmed_name.is_empty()
                    && self
                        .templates
                        .iter()
                        .any(|template| template.name.eq_ignore_ascii_case(trimmed_name));
                if overwrites_existing {
                    ui.label(egui::RichText::new("⚠").color(egui::Color32::YELLOW))
                        .on_hover_text("Saving will overwrite the template with this name.");
                }
                let button_text = if overwrites_existing {
                    "Update"
                } else {
                    "Save"
                };
                if ui.button(button_text).clicked() {
                    self.save_template();
                }
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.template_name)
                    .hint_text("Name for new template")
                    .desired_width(ui.available_width()),
            );
            if let Some(message) = &self.template_message {
                ui.small(message);
            }

            ui.horizontal(|ui| {
                ui.heading("Modifier Stack");
                ui.label(format!("({})", self.img_editor.pipeline.len()));
                if !self.img_editor.pipeline.is_empty() && ui.small_button("Clear all").clicked() {
                    self.img_editor.pipeline.clear();
                    self.dirty = true;
                }
            });

            ui.separator();
            if self.img_editor.pipeline.is_empty() {
                ui.label("No modifiers yet. Add an effect above.");
            }

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
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            handle.ui(ui, |ui| {
                                ui.label("::");
                            });
                            let title = match &item.effect {
                                EffectType::Blur { .. } => "Blur",
                                EffectType::Brightness { .. } => "Brightness",
                                EffectType::Contrast { .. } => "Contrast",
                                EffectType::Watermark { .. } => "Text watermark",
                            };
                            ui.strong(title);
                            if ui.small_button("Remove").clicked() {
                                remove_index = Some(state.index);
                            }
                        });

                        match &mut item.effect {
                            EffectType::Blur { sigma } => {
                                if ui.add(egui::Slider::new(sigma, 0.0..=10.0)).changed() {
                                    self.dirty = true;
                                }
                            }
                            EffectType::Brightness { value } => {
                                if ui.add(egui::Slider::new(value, -100..=100)).changed() {
                                    self.dirty = true;
                                }
                            }
                            EffectType::Contrast { value } => {
                                if ui.add(egui::Slider::new(value, 0.0..=5.0)).changed() {
                                    self.dirty = true;
                                }
                            }
                            EffectType::Watermark { params } => {
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
                    });
                },
            );

            if let Some(idx) = remove_index {
                self.img_editor.pipeline.remove(idx);
                self.dirty = true;
            }

            if response.final_update().is_some() {
                self.dirty = true;
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.dirty {
                self.request_render(ctx);
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
            let status_row_size = egui::vec2(ui.available_width(), 24.0);
            ui.allocate_ui_with_layout(
                status_row_size,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if self.rendering {
                        ui.spinner();
                        ui.label("Processing image...");
                    } else if let Some(error) = &self.error_message {
                        ui.colored_label(egui::Color32::RED, error);
                    }
                },
            );

            if let Some(texture) = &self.display_texture {
                let available_width = ui.available_width();
                let image_width = self.img_editor.original_image.width() as f32;
                let fit_width = available_width.min(image_width).max(1.0);
                let display_width = fit_width * self.zoom_percent / 100.0;
                let aspect_ratio = texture.size_vec2().y / texture.size_vec2().x;
                let display_size = egui::vec2(display_width, display_width * aspect_ratio);
                let watermark =
                    self.img_editor
                        .pipeline
                        .iter()
                        .rev()
                        .find_map(|op| match &op.effect {
                            EffectType::Watermark { params } => Some((op.id, params.x, params.y)),
                            _ => None,
                        });
                let mut drag_delta = None;

                egui::ScrollArea::both().show(ui, |ui| {
                    let image_response = ui.add(
                        egui::Image::new((texture.id(), display_size)).sense(egui::Sense::hover()),
                    );
                    if let Some((id, x, y)) = watermark {
                        let image_scale =
                            display_width / self.img_editor.original_image.width() as f32;
                        let handle_center = image_response.rect.min
                            + display_size * 0.5
                            + egui::vec2(x as f32, y as f32) * image_scale;
                        let handle_rect =
                            egui::Rect::from_center_size(handle_center, egui::vec2(32.0, 32.0));
                        let handle = ui
                            .interact(
                                handle_rect,
                                ui.id().with(("watermark_drag", id)),
                                egui::Sense::drag(),
                            )
                            .on_hover_text("Drag to reposition the watermark");
                        let painter = ui.painter();
                        painter.circle_filled(handle_center, 11.0, egui::Color32::from_gray(24));
                        painter.circle_stroke(
                            handle_center,
                            9.0,
                            egui::Stroke::new(2.0_f32, egui::Color32::WHITE),
                        );
                        painter.line_segment(
                            [
                                handle_center - egui::vec2(5.0, 0.0),
                                handle_center + egui::vec2(5.0, 0.0),
                            ],
                            egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                        );
                        painter.line_segment(
                            [
                                handle_center - egui::vec2(0.0, 5.0),
                                handle_center + egui::vec2(0.0, 5.0),
                            ],
                            egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
                        );
                        if handle.dragged() {
                            drag_delta = Some(handle.drag_delta() / image_scale);
                        }
                    }
                });

                if let (Some((id, _, _)), Some(delta)) = (watermark, drag_delta) {
                    if let Some(op) = self.img_editor.pipeline.iter_mut().find(|op| op.id == id) {
                        if let EffectType::Watermark { params } = &mut op.effect {
                            let max_x = (self.img_editor.original_image.width() / 2) as i32;
                            let max_y = (self.img_editor.original_image.height() / 2) as i32;
                            params.x = (params.x + delta.x.round() as i32).clamp(-max_x, max_x);
                            params.y = (params.y + delta.y.round() as i32).clamp(-max_y, max_y);
                            self.dirty = true;
                        }
                    }
                }
            }
        });
    }
}
