//! Generally useful shared code.

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
