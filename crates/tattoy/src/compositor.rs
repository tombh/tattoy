//! Composite individual cells into the final renderablsee frame.
use color_eyre::eyre::{ContextCompat as _, Result};

/// Composite cells together, honouring alpha blending, text and pixels.
#[derive(Default)]
pub(crate) struct Compositor;

impl Compositor {
    /// Get a mutable reference to a cell.
    pub fn get_cell_mut<'cell>(
        cells: &'cell mut [&mut [termwiz::cell::Cell]],
        x: usize,
        y: usize,
    ) -> Result<&'cell mut termwiz::cell::Cell> {
        let x_message = Self::no_coord_error_message("x", x);
        let y_message = Self::no_coord_error_message("y", y);
        cells
            .get_mut(y)
            .context(y_message)?
            .get_mut(x)
            .context(x_message)
    }

    /// Get a reference to a cell.
    pub fn get_cell<'cell>(
        cells: &'cell [&mut [termwiz::cell::Cell]],
        x: usize,
        y: usize,
    ) -> Result<&'cell termwiz::cell::Cell> {
        let x_message = Self::no_coord_error_message("x", x);
        let y_message = Self::no_coord_error_message("y", y);
        cells.get(y).context(y_message)?.get(x).context(x_message)
    }

    /// The error message when a cell doesn't exist at the provided coordinate.
    fn no_coord_error_message(axis: &str, coord: usize) -> String {
        format!("No {axis} coord ({coord}) for cell")
    }

    /// Simply use the incoming cell's foreground colour for the base cell's foreground
    /// colour.
    pub fn composite_fg_colour_only(
        base_cell: &mut termwiz::cell::Cell,
        cell_above: &termwiz::cell::Cell,
    ) {
        if base_cell
            .str()
            .chars()
            .all(|character| character.is_whitespace() || character == '▀' || character == '▄')
        {
            return;
        }

        let mut draft = termwiz::cell::Cell::blank();
        Self::composite_cells(&mut draft, cell_above, 1.0);
        let colour = draft.attrs().foreground();
        base_cell.attrs_mut().set_foreground(colour);
    }

    /// Composite 2 cells together.
    pub fn composite_cells(
        composited_cell: &mut termwiz::cell::Cell,
        cell_above: &termwiz::cell::Cell,
        opacity: f32,
    ) {
        let character_above = cell_above.str().to_owned();
        let is_character_above_text = !character_above.is_empty() && character_above != " ";
        if is_character_above_text {
            // All this is just because there doesn't seem to be a `cell.set_str("f")` method.
            let old_background = composited_cell.attrs().background();
            let old_foreground = composited_cell.attrs().foreground();
            *composited_cell = cell_above.clone();
            composited_cell.attrs_mut().set_background(old_background);
            composited_cell.attrs_mut().set_foreground(old_foreground);
        }

        let mut opaque = crate::opaque_cell::OpaqueCell::new(composited_cell, None, opacity);
        opaque.blend_all(cell_above);
    }

    /// Automatically adjust text contrast.
    pub fn auto_text_contrast(
        composited_cell: &mut termwiz::cell::Cell,
        target_text_contrast: f32,
        apply_to_readable_text_only: bool,
    ) {
        let mut opaque = crate::opaque_cell::OpaqueCell::new(composited_cell, None, 1.0);
        opaque.ensure_readable_contrast(target_text_contrast, apply_to_readable_text_only);
    }

    /// Add a little indicator in the top-right to show that Tattoy is running.
    pub fn add_indicator(
        cells: &mut [&mut [termwiz::cell::Cell]],
        indicator_cell: &termwiz::cell::Cell,
        x: usize,
        y: usize,
    ) -> Result<()> {
        let composited_cell = Self::get_cell_mut(cells, x, y)?;
        Self::composite_cells(composited_cell, indicator_cell, 1.0);

        Ok(())
    }
}
