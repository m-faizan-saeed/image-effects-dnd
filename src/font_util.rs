
use ab_glyph::{FontVec, PxScaleFont, ScaleFont};


pub(crate) fn calculate_line_width(scaled_font: PxScaleFont<&FontVec>, text: &str) -> f32 {
    let mut width = 0.0;
    let mut last_glyph_id = None;

    for c in text.chars() {
        let glyph_id = scaled_font.glyph_id(c);

        // 2. Kerning exists on the 'Font' trait (unscaled)
        if let Some(last_id) = last_glyph_id {
            // Important: Unscaled kern units must be multiplied by the scale factor
            // or use the helper: scaled_font.kern(last_id, glyph_id)
            width += scaled_font.kern(last_id, glyph_id);
        }

        // 3. Add the advance width (already scaled)
        width += scaled_font.h_advance(glyph_id);
        last_glyph_id = Some(glyph_id);
    }
    width
}