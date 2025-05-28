//! Test helpers

#![expect(clippy::unwrap_used, reason = "It's for use in tests only")]

/// Get the path to the root of the Cargo workspace.
///
/// # Panics
#[inline]
pub fn workspace_dir() -> std::path::PathBuf {
    let output = std::process::Command::new(env!("CARGO"))
        .arg("locate-project")
        .arg("--workspace")
        .arg("--message-format=plain")
        .output()
        .unwrap()
        .stdout;
    let cargo_path = std::path::Path::new(std::str::from_utf8(&output).unwrap().trim());
    let workspace_dir = cargo_path.parent().unwrap().to_path_buf();
    tracing::debug!("Using workspace directory: {workspace_dir:?}");
    workspace_dir
}

/// Define a canonical shell that is a consistent as possible. Useful for end to end testing.
#[inline]
#[must_use]
pub fn get_canonical_shell() -> Vec<std::ffi::OsString> {
    #[cfg(not(target_os = "windows"))]
    let mut shell = "bash --norc --noprofile".to_owned();

    #[cfg(target_os = "windows")]
    let mut shell = "powershell -NoProfile".to_owned();

    if let Ok(custom_shell) = std::env::var("CANONICAL_SHELL") {
        shell = custom_shell;
    }

    tracing::debug!("Use canonical shell: {shell}");

    shell
        .split_whitespace()
        .map(std::convert::Into::into)
        .collect()
}

/// Run a steppable terminal.
///
/// # Panics
#[inline]
pub async fn run(
    width: Option<u16>,
    height: Option<u16>,
) -> crate::steppable_terminal::SteppableTerminal {
    let config = crate::shadow_terminal::Config {
        width: width.unwrap_or(50),
        height: height.unwrap_or(10),
        command: get_canonical_shell(),
        ..crate::shadow_terminal::Config::default()
    };
    Box::pin(crate::steppable_terminal::SteppableTerminal::start(config))
        .await
        .unwrap()
}
