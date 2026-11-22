//! The app's one-line text field.

use eframe::egui::{Align2, TextBuffer, TextEdit};

/// egui's one-line field with its text centred in the frame: egui puts the text at the top of a
/// frame as high as a button, which sat the text and the cursor above the middle.
pub fn text_field(text: &mut dyn TextBuffer) -> TextEdit<'_> {
    TextEdit::singleline(text).align(Align2::LEFT_CENTER)
}
