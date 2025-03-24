//! All the CLI arguments for Tattoy

/// The default name of the main config file.
pub const DEFAULT_CONFIG_FILE_NAME: &str = "tattoy.toml";

/// Simple program to greet a person
#[derive(clap::Parser, Debug, Clone)]
#[command(version, about, long_about = "Tattoy argument description")]
pub(crate) struct CliArgs {
    /// Name of the Tattoy(s) to use.
    #[arg(long("use"))]
    pub enabled_tattoys: Vec<String>,

    // TODO: Currently only usesd by the e2e tests. I'd rather have a more general purpose flag
    // that allowed overriding any config use a classic dot notation:
    // `config.minimap.enabled = false`.
    //
    /// The command to start Tattoy with. Default to `$SHELL`.
    #[arg(long)]
    pub command: Option<String>,

    /// Use image capture to detect the true colour values of the terminal's palette.
    #[arg(long)]
    pub capture_palette: bool,

    /// Provide a screenshot of the terminal's palette for parsing into true colours.
    #[arg(long, value_name = "Path to screenshot file")]
    pub parse_palette: Option<String>,

    /// Path to config file directory. A directory must be used because Tattoy has various config
    /// files.
    #[arg(long, value_name = "Path to config directory")]
    pub config_dir: Option<std::path::PathBuf>,

    /// Override the default Tattoy config *file*. The same default config directory is used, so the
    /// palette and shader files are the same.
    #[arg(
        long,
        default_value = DEFAULT_CONFIG_FILE_NAME,
        value_name = "Path to the main Tattoy config file"
    )]
    pub main_config: std::path::PathBuf,

    /// Path to the log file, overrides the setting in config.
    #[arg(long, value_name = "Path to log file")]
    pub log_path: Option<std::path::PathBuf>,

    /// Verbosity of logs
    #[arg(long, value_name = "Level to log at")]
    pub log_level: Option<crate::config::LogLevel>,
}
