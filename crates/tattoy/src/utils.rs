//! Generally useful shared code.

#[cfg(not(target_os = "windows"))]
/// The Unix newline
pub const NEWLINE: &str = "\n";

#[cfg(target_os = "windows")]
/// The Windows newline
pub const NEWLINE: &str = "\r\n";
