//! Setters that leave a widget alone when it already shows what it is given.
//! xui re-flows the whole window after any label's text or combo box's choice
//! is set, changed or not, and the panels are synced on every pointer move of a
//! drag: on the showcase that re-flow was 15 to 50 ms a step.

use xui::widget::{ComboBox, HasText};

pub fn set_text(widget: &impl HasText, text: &str) {
    if widget.text() != text {
        widget.set_text(text);
    }
}

pub fn select<M: 'static>(combo: &ComboBox<M>, index: usize) {
    if combo.selected() != index {
        combo.select(index);
    }
}
