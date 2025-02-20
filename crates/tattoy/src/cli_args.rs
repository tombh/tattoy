//! All the CLI arguments for Tattoy

/// Simple program to greet a person
#[derive(clap::Parser, Debug, Clone)]
#[command(version, about, long_about = "Tattoy argument description")]
#[non_exhaustive]
pub struct CliArgs {
    /// Name of the Tattoy(s) to use.
    #[arg(short, long("use"))]
    pub enabled_tattoys: Vec<String>,

    /// The command to start Tattoy with. Default to `$SHELL`.
    #[arg(short, long)]
    pub command: Option<String>,

    /// Use image capture to detect the true colour values of the terminal's palette.
    #[arg(long)]
    pub capture_palette: bool,

    /// Provide a screenshot of the terminal's palette for parsing into true colours.
    #[arg(long, value_name = "Path to screenshot file")]
    pub parse_palette: Option<String>,
}
