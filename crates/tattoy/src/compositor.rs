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
        let character_above = cell_above.str();
        let is_composited_cell_pixel = composited_cell.str() == "▀" || composited_cell.str() == "▄";
        let is_character_above_pixel = character_above == "▀" || character_above == "▄";
        let is_character_above_empty = character_above.is_empty() || character_above == " ";
        let is_character_above_text = !is_character_above_empty && !is_character_above_pixel;
        let is_pixel_onto_non_pixel = is_character_above_pixel && !is_composited_cell_pixel;

        if is_character_above_text || is_pixel_onto_non_pixel {
            *composited_cell = termwiz::cell::Cell::new(
                character_above.chars().nth(0).unwrap_or(' '),
                composited_cell.attrs().clone(),
            );
        }

        let mut blender = crate::blender::Blender::new(composited_cell, None, opacity);
        blender.blend_all(cell_above);

        // The convention we use for pixel graphics is that we always try to render using the upper
        // half block. But there is one edge case were we have to use a lower half block. So in the
        // case where we composite a lower half onto an upper half we are actually escaping that edge
        // case so we can return back to the convention of defaulting to the upper half block.
        if composited_cell.str() == "▄" && character_above == "▀" {
            *composited_cell = termwiz::cell::Cell::new('▀', composited_cell.attrs().clone());
        }
    }

    /// Automatically adjust text contrast.
    pub fn auto_text_contrast(
        composited_cell: &mut termwiz::cell::Cell,
        target_text_contrast: f32,
        apply_to_readable_text_only: bool,
    ) {
        let mut blender = crate::blender::Blender::new(composited_cell, None, 1.0);
        blender.ensure_readable_contrast(target_text_contrast, apply_to_readable_text_only);
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

    // TODO: This doesn't handle the case where there are actual legitimate half-blocks under the
    // cursor. Consider the case of editing this very function in Tattoy, the "▄"s and "▀"s will
    // dissapear when the cursor is over them. Perhaps only do this when the cursor shape is a
    // block?
    //
    /// Ensure that the cursor shape doesn't conflict with any pixels below.
    pub fn clean_cursor_cell(
        cells: &mut [&mut [termwiz::cell::Cell]],
        cursor_x: usize,
        cursor_y: usize,
    ) {
        let maybe_cell = Self::get_cell_mut(cells, cursor_x, cursor_y);
        let Ok(composited_cell) = maybe_cell else {
            tracing::warn!("Couldn't get cell under cursor at: {cursor_x}x{cursor_y}");
            return;
        };

        if composited_cell.str() == "▄" || composited_cell.str() == "▀" {
            *composited_cell = termwiz::cell::Cell::new(' ', composited_cell.attrs().clone());
        }
    }
}
