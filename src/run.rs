//! Docs

use std::process::exit;

use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use crate::pty::{StreamBytes, PTY};
use crate::renderer::Renderer;
use crate::shadow_tty::ShadowTTY;
use crate::tattoys::Tattoys;

/// There a are 2 "screens" or "surfaces" to manage in Tattoy. The fancy special affects screen
/// and the traditional PTY.
#[non_exhaustive]
pub enum SurfaceType {
    /// A frame of tattoy screen
    BGSurface,
    /// A frame of a PTY terminal
    PTYSurface,
}

/// The message type of the output channel. We want to be able to react immediately to either
/// new PTY data or new fancy Tattoy data.
#[non_exhaustive]
pub struct TattoySurface {
    /// The type of the surface
    pub kind: SurfaceType,
    /// The surface data itself. It's lots of "cells", each with colour
    /// attributes and a character.
    pub surface: termwiz::surface::Surface,
}

/// Docs
#[allow(
    clippy::print_stdout,
    clippy::wildcard_enum_match_arm,
    clippy::use_debug
)]
pub fn run() -> Result<()> {
    let log_file = "tattoy.log";
    let file = std::fs::File::create(log_file)?;
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_env_filter(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();
    tracing::info!("Starting Tattoy");

    let (pty_output_tx, pty_output_rx) = mpsc::unbounded_channel::<StreamBytes>();
    let (pty_input_tx, pty_input_rx) = mpsc::unbounded_channel::<StreamBytes>();
    let (bg_screen_tx, screen_rx) = mpsc::unbounded_channel();
    let pty_screen_tx = bg_screen_tx.clone();

    let mut renderer = Renderer::new()?;

    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::as_conversions)]
    let pty = PTY::new(
        renderer.height as u16,
        renderer.width as u16,
        vec![std::env::var("SHELL")?.into()],
    )?;

    std::thread::spawn(move || PTY::consume_stdin(&pty_input_tx));

    tokio::spawn(async move {
        let mut shadow = ShadowTTY::new(renderer.height, renderer.width);
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        #[allow(clippy::print_stderr)]
        #[allow(clippy::exit)]
        if let Err(err) = shadow.run(pty_output_rx, &pty_screen_tx).await {
            eprintln!("{err}");
            exit(1);
        }
    });

    // Use a thread because it's likely more CPU bound
    std::thread::spawn(move || {
        let tattoys = Tattoys::new(renderer.width, renderer.height);
        #[allow(clippy::print_stderr)]
        #[allow(clippy::exit)]
        if let Err(err) = tattoys.run(&bg_screen_tx) {
            eprintln!("{err}");
            exit(1);
        }
    });

    std::thread::spawn(move || {
        #[allow(clippy::print_stderr)]
        #[allow(clippy::exit)]
        if let Err(err) = renderer.run(screen_rx) {
            eprintln!("{err}");
            exit(1);
        }
    });

    pty.run(pty_input_rx, pty_output_tx)?;

    Ok(())
}
