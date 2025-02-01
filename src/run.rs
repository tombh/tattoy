//! Main entrypoint for running Tattoy

use std::process::exit;
use std::sync::Arc;

use clap::Parser as _;
use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use crate::cli_args::CliArgs;
use crate::loader::Loader;
use crate::pty::{StreamBytesFromPTY, StreamBytesFromSTDIN, PTY};
use crate::renderer::Renderer;
use crate::shadow_tty::ShadowTTY;
use crate::shared_state::SharedState;

// TODO: Maybe it'd be nice to also just send an vector of true colour pixels? Like a frame of a
// video for example?
//
/// There a are 2 "screens" or "surfaces" to manage in Tattoy. The fancy special affects screen
/// and the traditional PTY.
#[non_exhaustive]
pub enum FrameUpdate {
    /// A frame of a tattoy TTY screen
    TattoySurface(termwiz::surface::Surface),
    /// A frame of a PTY terminal has been updated in the shared state
    PTYSurface,
}

/// Commands to control the various tasks/threads
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum Protocol {
    /// The entire application is exiting
    End,
    /// User's TTY is resized.
    Resize {
        /// Width of new terminal
        width: u16,
        /// Height of new terminal
        height: u16,
    },
}

/// Main entrypoint
#[expect(
    clippy::use_debug,
    clippy::print_stderr,
    clippy::exit,
    reason = "The central place where errors are handled"
)]
pub(crate) async fn run() -> Result<()> {
    let mut enabled_tattoys: Vec<String> = vec![];

    // Assuming true colour makes Tattoy simpler.
    // * I think it's safe to assume that the vast majority of people using Tattoy will have a
    //   true color terminal anyway.
    // * Even if a user doesn't have a true colour terminal, we should be able to internally
    //   render as true color and then downgrade later when Tattoy does its final output.
    std::env::set_var("COLORTERM", "truecolor");

    tracing::info!("Starting Tattoy");

    let cli = CliArgs::parse();
    if let Some(tattoys) = cli.enabled_tattoys {
        enabled_tattoys.push(tattoys);
    } else {
        eprintln!("Please provide at least one tattoy with the `--use` argument");
        exit(1);
    }

    let state_arc = SharedState::init()?;

    let (pty_output_tx, pty_output_rx) = mpsc::channel::<StreamBytesFromPTY>(1);
    let (pty_input_tx, pty_input_rx) = mpsc::channel::<StreamBytesFromSTDIN>(1);

    // TODO: Channel size of 16 caused `no available capacity` error. Think about what is an actual
    // reasonable size, or handle the error more gracefully.
    let (bg_screen_tx, screen_rx) = mpsc::channel(8192);
    let pty_screen_tx = bg_screen_tx.clone();

    let (protocol_tx, _) = tokio::sync::broadcast::channel(64);
    let protocol_stdin_rx = protocol_tx.subscribe();
    let protocol_pty_rx = protocol_tx.subscribe();
    let protocol_shadow_rx = protocol_tx.subscribe();
    let protocol_runner_rx = protocol_tx.subscribe();
    let protocol_renderer_rx = protocol_tx.subscribe();

    let tty_size = crate::renderer::Renderer::get_users_tty_size()?;
    state_arc.set_tty_size(tty_size.cols.try_into()?, tty_size.rows.try_into()?)?;

    let pty = PTY::new(vec![std::env::var("SHELL")?.into()])?;

    tokio::spawn(async move {
        if let Err(err) = PTY::consume_stdin(&pty_input_tx, protocol_stdin_rx).await {
            eprintln!("PTY error: {err:?}");
            exit(1);
        };
    });

    let shadow_state = Arc::clone(&state_arc);
    tokio::spawn(async move {
        let mut shadow_tty = match ShadowTTY::new(shadow_state) {
            Ok(ok) => ok,
            Err(err) => {
                eprintln!("Shadow TTY error: {err:?}");
                exit(1);
            }
        };

        if let Err(err) = shadow_tty
            .run(pty_output_rx, &pty_screen_tx, protocol_shadow_rx)
            .await
        {
            eprintln!("Shadow TTY error: {err:?}");
            exit(1);
        }
    });

    let loader_state = Arc::clone(&state_arc);
    let loader_thread = std::thread::spawn(move || {
        let maybe_tattoys = Loader::new(&loader_state, enabled_tattoys);
        match maybe_tattoys {
            Ok(mut tattoys) => {
                if let Err(err) = tattoys.run(&bg_screen_tx, protocol_runner_rx) {
                    // Note: I've seen `no available capacity`
                    eprintln!("Tattoy runner error: {err:?}");
                    exit(1);
                }
            }
            Err(err) => {
                eprintln!("Tattoys Loader error: {err:?}");
                exit(1);
            }
        }
    });

    let protocol_tx_clone = protocol_tx.clone();
    let render_state = Arc::clone(&state_arc);
    let render_task = tokio::spawn(async move {
        let maybe_renderer = Renderer::new(render_state);
        match maybe_renderer {
            Ok(mut renderer) => {
                if let Err(err) = renderer
                    .run(screen_rx, protocol_renderer_rx, protocol_tx_clone)
                    .await
                {
                    eprintln!("Renderer error: {err:?}");
                    exit(1);
                };
            }
            Err(err) => {
                eprintln!("Tattoys Loader error: {err:?}");
                exit(1);
            }
        };
    });

    pty.run(pty_input_rx, pty_output_tx, protocol_pty_rx)
        .await?;
    protocol_tx.send(Protocol::End)?;

    if let Err(err) = loader_thread.join() {
        eprintln!("Couldn't join loader thread: {err:?}");
        exit(1);
    };

    if let Err(err) = render_task.await {
        eprintln!("Couldn't join render task: {err:?}");
        exit(1);
    };

    Ok(())
}
