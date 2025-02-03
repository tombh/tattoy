//! Main entrypoint for running Tattoy

use std::sync::Arc;

use clap::Parser as _;
use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use crate::cli_args::CliArgs;
use crate::input::Input;
use crate::loader::Loader;
use crate::pty::PTY;
use crate::renderer::Renderer;
use crate::shadow_tty::ShadowTTY;
use crate::shared_state::SharedState;

// TODO: Maybe it'd be nice to also just send a vector of true colour pixels? Like a frame of a
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

// TODO:
// Putting any errors in shared state, feels a bit weird. Does it make more sense to have each task/thread
// return its error, and then check them all at the end?
//
/// Main entrypoint
pub(crate) async fn run(state_arc: &std::sync::Arc<SharedState>) -> Result<()> {
    let enabled_tattoys = setup(state_arc)?;
    let (protocol_tx, _) = tokio::sync::broadcast::channel(64);

    // TODO: I've seen this cause "No more capacity" error.
    let (pty_input_tx, pty_input_rx) = mpsc::channel(1);
    let input_handle = Input::start(pty_input_tx, protocol_tx.clone());

    let (pty_output_tx, pty_output_rx) = mpsc::channel(1);
    let (tattoy_frame_tx, tattoy_frame_rx) = mpsc::channel(8192);
    let shadow_tty_handle = ShadowTTY::start(
        Arc::clone(state_arc),
        protocol_tx.clone(),
        pty_output_rx,
        tattoy_frame_tx.clone(),
    );

    let tattoys_handle = Loader::start(
        enabled_tattoys,
        Arc::clone(state_arc),
        protocol_tx.clone(),
        tattoy_frame_tx,
    );

    let renderer = Renderer::start(Arc::clone(state_arc), tattoy_frame_rx, protocol_tx.clone());

    let pty = PTY::new(vec![std::env::var("SHELL")?.into()])?;
    pty.run(
        pty_input_rx,
        pty_output_tx,
        protocol_tx.subscribe(),
        protocol_tx.subscribe(),
    )
    .await?;

    tracing::debug!("Left PTY thread, exiting Tattoy...");
    protocol_tx.send(Protocol::End)?;

    tattoys_handle
        .join()
        .map_err(|err| color_eyre::eyre::eyre!("{err:?}"))??;

    if input_handle.is_finished() {
        // The STDIN loop doesn't listen to the global Tattoy protocol, so it can't exit its loop.
        // Therefore we should only join it if it finished because of its own error.
        input_handle
            .join()
            .map_err(|err| color_eyre::eyre::eyre!("{err:?}"))??;
    }

    shadow_tty_handle.await??;
    renderer.await??;

    Ok(())
}

/// Signal all task/thread loops to exit.
///
/// We keep it in its own function because we need to handle the error separately. If the error
/// were to be bubbled with `?` as usual, there's a chance it would never be logged, because the
/// protocol end signal is itself what allows the central error handler to even be reached.
pub fn broadcast_protocol_end(protocol_tx: &tokio::sync::broadcast::Sender<Protocol>) {
    tracing::debug!("Broadcasting the protocol `End` message to all listeners");
    let result = protocol_tx.send(Protocol::End);
    if let Err(error) = result {
        tracing::error!("{error:?}");
    }
}

/// Prepare the application to start.
fn setup(state: &std::sync::Arc<SharedState>) -> Result<Vec<String>> {
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
        let error =
            color_eyre::eyre::eyre!("Please provide at least one tattoy with the `--use` argument");
        return Err(error);
    }

    let tty_size = crate::renderer::Renderer::get_users_tty_size()?;
    state.set_tty_size(tty_size.cols.try_into()?, tty_size.rows.try_into()?)?;

    Ok(enabled_tattoys)
}
