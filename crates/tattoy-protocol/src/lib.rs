//! These are all the types that a plugin needs to function.

#![expect(clippy::pub_use, reason = "This seems to come from the `bon` crate")]

/// An RGBA colour.
pub type Colour = (f32, f32, f32, f32);

/// A cell represents a single character in the terminal.
///
/// It can be sent from Tattoy to communicate the contents of the user's terminal.
/// And it can also be sent from a plugin to communicate the contents to be composited
/// in a Tattoy layer.
#[derive(serde::Serialize, serde::Deserialize, bon::Builder, Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Cell {
    /// The cell's character.
    pub character: char,
    /// The coordinates of the cell. [0, 0] is in the top-left.
    pub coordinates: (u32, u32),
    /// An optional colour for the cell's background. If `None` (or `null` in the case of JSON) is
    /// used then the terminal's default background colour will be used.
    pub bg: Option<Colour>,
    /// An optional colour for the cell's foreground. If `None` (or `null` in the case of JSON) is
    /// used then the terminal's default foreground colour will be used.
    pub fg: Option<Colour>,
}

/// Output from the plugin that renders pixels in the terminal.
#[derive(serde::Serialize, serde::Deserialize, bon::Builder, Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Pixel {
    /// The coordinates of the pixel. [0, 0] is in the top-left. The y-axis is twice as long as the
    /// number of rows in the terminal because 2 "pixels" can fit in a single TTY cell using the
    /// UTF8 half-block trick: ▀▄▀▄
    pub coordinates: (u32, u32),
    /// An optional colour for the pixel. If `None` (or `null` in the case of JSON) is used then
    /// the default foreground colour is used.
    pub color: Option<Colour>,
}

/// The various kinds of messages that Tattoy can send to the plugin.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PluginInputMessages {
    /// The current contents of the PTY screen. It does not contain any of the scrollback.
    #[serde(rename = "pty_update")]
    PTYUpdate {
        /// The size of terminal in colums and rows.
        size: (u16, u16),
        /// All the cell data for the current terminal. Blank cells are not included.
        cells: Vec<Cell>,
        /// The current position of the cursor.
        cursor: (u16, u16),
    },
    /// Sent whenever the terminal resizes.
    #[serde(rename = "tty_resize")]
    TTYResize {
        /// The number of columns in the new terminal size.
        width: u16,
        /// The number of rows in the new terminal size.
        height: u16,
    },
}

/// All the message kinds that the plugin can send to Tattoy.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PluginOutputMessages {
    /// Output from the plugin that renders text of arbitrary length in the terminal.
    OutputText {
        /// The text to display.
        text: String,
        /// The coordinates. [0, 0] is in the top-left.
        coordinates: (u32, u32),
        /// An optional colour for the text's background.
        bg: Option<Colour>,
        /// An optional colour for the text's foreground.
        fg: Option<Colour>,
    },

    /// Output an arbitrary amount of cells to the terminal. It does not need to include blank
    /// cells.
    OutputCells(Vec<Cell>),

    /// Output from the plugin that renders pixels in the terminal.
    OutputPixels(Vec<Pixel>),
}

#[expect(clippy::default_numeric_fallback, reason = "Tests aren't so strict")]
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn output_text() {
        let expected = serde_json::json!(
            {
                "output_text": {
                    "text": "foo",
                    "coordinates": [1, 2],
                    "bg": null,
                    "fg": [0.1, 0.2, 0.3, 0.4],
                }
            }
        );

        let output = PluginOutputMessages::OutputText {
            text: "foo".to_owned(),
            coordinates: (1, 2),
            bg: None,
            fg: Some((0.1, 0.2, 0.3, 0.4)),
        };

        assert_eq!(
            expected.to_string(),
            serde_json::to_string(&output).unwrap()
        );
    }

    #[test]
    fn output_cells() {
        let expected = serde_json::json!(
            {
                "output_cells": [{
                    "character": "f",
                    "coordinates": [1, 2],
                    "bg": null,
                    "fg": [0.1, 0.2, 0.3, 0.4],
                }]
            }
        );

        let output = PluginOutputMessages::OutputCells(vec![Cell {
            character: 'f',
            coordinates: (1, 2),
            bg: None,
            fg: Some((0.1, 0.2, 0.3, 0.4)),
        }]);

        assert_eq!(
            expected.to_string(),
            serde_json::to_string(&output).unwrap()
        );
    }

    #[test]
    fn output_pixels() {
        let expected = serde_json::json!(
            {
                "output_pixels": [{
                    "coordinates": [1, 2],
                    "color": [0.1, 0.2, 0.3, 0.4],
                }]
            }
        );

        let output = PluginOutputMessages::OutputPixels(vec![Pixel {
            coordinates: (1, 2),
            color: Some((0.1, 0.2, 0.3, 0.4)),
        }]);

        assert_eq!(
            expected.to_string(),
            serde_json::to_string(&output).unwrap()
        );
    }

    #[test]
    fn input_pty_update() {
        let expected = serde_json::json!(
            {
                "pty_update": {
                    "size": [1, 2],
                    "cells": [{
                        "character": "f",
                        "coordinates": [1, 2],
                        "bg": null,
                        "fg": [0.1, 0.2, 0.3, 0.4],
                    }],
                    "cursor": [9, 10],
                }
            }
        );

        let output = PluginInputMessages::PTYUpdate {
            size: (1, 2),
            cells: vec![Cell {
                character: 'f',
                coordinates: (1, 2),
                bg: None,
                fg: Some((0.1, 0.2, 0.3, 0.4)),
            }],
            cursor: (9, 10),
        };

        assert_eq!(
            expected.to_string(),
            serde_json::to_string(&output).unwrap()
        );
    }

    #[test]
    fn input_tty_resize() {
        let expected = serde_json::json!(
            {
                "tty_resize": {
                    "width": 1,
                    "height": 2,
                }
            }
        );

        let output = PluginInputMessages::TTYResize {
            width: 1,
            height: 2,
        };

        assert_eq!(
            expected.to_string(),
            serde_json::to_string(&output).unwrap()
        );
    }
}
