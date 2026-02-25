use crate::image_editor_ui::ImageEditorUi;

mod font_util;
mod image_editor;
mod image_editor_ui;
mod imageproc_util;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Linear Image Editor",
        options,
        Box::new(|cc| Ok(Box::new(ImageEditorUi::new(cc)))),
    )
}
