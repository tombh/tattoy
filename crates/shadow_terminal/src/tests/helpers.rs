//! asasdasdasd

/// asddasdasd
///
/// # Panics
/// basdasda
#[expect(clippy::unwrap_used, reason = "It's for use in tests only")]
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
