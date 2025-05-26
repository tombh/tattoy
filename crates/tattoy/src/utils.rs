//! Generally useful shared code.

/// The official Tattoy blue;
pub const TATTOY_BLUE: &str = "#0034a1";

#[cfg(not(target_os = "windows"))]
/// The Unix newline
pub const NEWLINE: &str = "\n";

#[cfg(target_os = "windows")]
/// The Windows newline
pub const NEWLINE: &str = "\r\n";

/// Reset any OSC colour codes
pub const RESET_COLOUR: &str = "\x1b[m";

/// OSC code to clear the terminal screen.
pub const CLEAR_SCREEN: &str = "\x1b[2J";

/// OSC code to reset the terminal screen.
pub const RESET_SCREEN: &str = "\x1bc";

/// Smoothly transition between 2 values.
#[must_use]
pub fn smoothstep(edge0: f32, edge1: f32, mut x: f32) -> f32 {
    x = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    x * x * 2.0f32.mul_add(-x, 3.0)
}
