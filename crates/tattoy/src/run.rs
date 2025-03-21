//! Main entrypoint for running Tattoy

use std::sync::Arc;

use clap::Parser as _;
use color_eyre::eyre::{ContextCompat as _, Result};
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _, Layer as _};

use crate::cli_args::CliArgs;
use crate::input::Input;
use crate::renderer::Renderer;
use crate::shared_state::SharedState;

// TODO:
//  * Can this not live on the protocol? Then we could get rid of the channel.
//  * Maybe it'd be nice to also just send a vector of true colour pixels? Like a frame of a
//    video for example?
//
/// There a are 2 "screens" or "surfaces" to manage in Tattoy. The fancy special affects screen
/// and the traditional PTY.
pub(crate) enum FrameUpdate {
    /// A frame of a tattoy TTY screen
    TattoySurface(crate::surface::Surface),
    /// A frame of a PTY terminal has been updated in the shared state
    PTYSurface,
}

/// Commands to control the various tasks/threads
#[non_exhaustive]
#[derive(Clone, Debug)]
pub(crate) enum Protocol {
    /// Output from the PTY.
    Output(shadow_terminal::output::Output),
    /// The entire application is exiting.
    End,
    /// User's TTY is resized.
    Resize {
        /// Width of new terminal.
        width: u16,
        /// Height of new terminal.
        height: u16,
    },
    /// Parsed input from STDIN.
    Input(crate::input::ParsedInput),
    /// The visibility of the end user's cursor.
    CursorVisibility(bool),
}

// TODO:
// Putting any errors in shared state, feels a bit weird. Does it make more sense to have each task/thread
// return its error, and then check them all at the end?
//
/// Main entrypoint
pub(crate) async fn run(state_arc: &std::sync::Arc<SharedState>) -> Result<()> {
    let cli_args = setup(state_arc).await?;

    if cli_args.capture_palette {
        crate::palette::parser::Parser::run(state_arc, None).await?;
        return Ok(());
    }

    if let Some(screenshot) = cli_args.parse_palette {
        crate::palette::parser::Parser::run(state_arc, Some(&screenshot)).await?;
        return Ok(());
    }

    let (protocol_tx, _) = tokio::sync::broadcast::channel(1024);
    let (surfaces_tx, surfaces_rx) = mpsc::channel(8192);

    let config_handle = crate::config::Config::watch(Arc::clone(state_arc), protocol_tx.clone());
    let input_thread_handle = Input::start(protocol_tx.clone());
    let tattoys_handle = crate::loader::start_tattoys(
        cli_args.enabled_tattoys.clone(),
        protocol_tx.clone(),
        surfaces_tx.clone(),
        Arc::clone(state_arc),
    );

    let renderer = Renderer::start(Arc::clone(state_arc), surfaces_rx, protocol_tx.clone());
    let users_tty_size = crate::renderer::Renderer::get_users_tty_size()?;
    crate::terminal_proxy::TerminalProxy::start(
        state_arc,
        surfaces_tx,
        protocol_tx.clone(),
        shadow_terminal::shadow_terminal::Config {
            width: users_tty_size.cols.try_into()?,
            height: users_tty_size.rows.try_into()?,
            command: get_startup_command(state_arc, cli_args).await?,
            ..Default::default()
        },
    )
    .await?;
    tracing::debug!("Left PTY thread, exiting Tattoy...");
    broadcast_protocol_end(&protocol_tx);

    tracing::trace!("Joining tattoys loader thread");
    tattoys_handle
        .join()
        .map_err(|err| color_eyre::eyre::eyre!("Tattoys handle: {err:?}"))??;

    tracing::trace!("Joining input thread");
    if input_thread_handle.is_finished() {
        // The STDIN loop doesn't listen to the global Tattoy protocol, so it can't exit its loop.
        // Therefore we should only join it if it finished because of its own error.
        input_thread_handle
            .join()
            .map_err(|err| color_eyre::eyre::eyre!("STDIN handle: {err:?}"))??;
    }

    tracing::trace!("Awaiting renderer task");
    renderer.await??;

    tracing::trace!("Awaiting config watcher task");
    config_handle.await??;

    tracing::trace!("Leaving Tattoy's main `run()` function");
    Ok(())
}

/// Get the command that Tattoy will use to startup, usually something like `bash`.
async fn get_startup_command(
    state: &std::sync::Arc<SharedState>,
    cli_args: CliArgs,
) -> Result<Vec<std::ffi::OsString>> {
    let maybe_cli_command = cli_args.command;
    let command = match maybe_cli_command {
        Some(cli_command) => cli_command,
        None => state.config.read().await.command.clone(),
    };

    let parts = command
        .split_whitespace()
        .map(std::convert::Into::into)
        .collect();

    tracing::debug!("Starting Tattoy with command: '{command:?}'");
    Ok(parts)
}

/// Signal all task/thread loops to exit.
///
/// We keep it in its own function because we need to handle the error separately. If the error
/// were to be bubbled with `?` as usual, there's a chance it would never be logged, because the
/// protocol end signal is itself what allows the central error handler to even be reached.
pub(crate) fn broadcast_protocol_end(protocol_tx: &tokio::sync::broadcast::Sender<Protocol>) {
    tracing::debug!("Broadcasting the protocol `End` message to all listeners");
    let result = protocol_tx.send(Protocol::End);
    if let Err(error) = result {
        tracing::error!("{error:?}");
    }
}

/// Prepare the application to start.
async fn setup(state: &std::sync::Arc<SharedState>) -> Result<CliArgs> {
    let cli_args = CliArgs::parse();
    crate::config::Config::setup_directory(cli_args.config_dir.clone(), state).await?;
    crate::config::Config::update_shared_state(state).await?;

    setup_logging(state).await?;

    // Assuming true colour makes Tattoy simpler.
    // * I think it's safe to assume that the vast majority of people using Tattoy will have a
    //   true color terminal anyway.
    // * Even if a user doesn't have a true colour terminal, we should be able to internally
    //   render as true color and then downgrade later when Tattoy does its final output.
    std::env::set_var("COLORTERM", "truecolor");

    // There's probably a better way of doing this, like just inheriting it from the user. But for
    // now always defaulting to "xterm-256color" fixes some bugs, namely mouse input in `htop`.
    let term = state.config.read().await.term.clone();
    tracing::debug!("Setting `TERM` env to: '{term}'");
    std::env::set_var("TERM", term);

    tracing::info!("Starting Tattoy");

    let tty_size = crate::renderer::Renderer::get_users_tty_size()?;
    state
        .set_tty_size(tty_size.cols.try_into()?, tty_size.rows.try_into()?)
        .await;

    Ok(cli_args)
}

/// Setup logging
async fn setup_logging(state: &std::sync::Arc<SharedState>) -> Result<()> {
    let path = state.config.read().await.log_path.clone();
    let directory = path.parent().context("Couldn't get log path's parent")?;
    std::fs::create_dir_all(directory)?;
    let file = std::fs::File::create(path)?;

    let level = state.config.read().await.log_level.clone();

    let logfile_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                // We don't want any of the trace lines that make the `tokio-console` possible
                .add_directive(format!("shadow_terminal={level}").parse()?)
                .add_directive(format!("tattoy={level}").parse()?)
                .add_directive(format!("tests={level}").parse()?)
                .add_directive("tokio=debug".parse()?)
                .add_directive("runtime=debug".parse()?),
        );

    let tracing_setup = tracing_subscriber::registry().with(logfile_layer);

    if std::env::var_os("ENABLE_TOKIO_CONSOLE") == Some("1".into()) {
        let console_layer = console_subscriber::spawn();
        tracing_setup.with(console_layer).init();
    } else {
        tracing_setup.init();
    }

    Ok(())
}
