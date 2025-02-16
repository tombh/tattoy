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
}
