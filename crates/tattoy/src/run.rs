//! Main entrypoint for running Tattoy

use std::sync::Arc;

use clap::Parser as _;
use color_eyre::eyre::{ContextCompat as _, Result};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _, Layer as _};

use crate::cli_args::CliArgs;
use crate::raw_input::RawInput;
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
    /// A signal to indicate that a system has successfully started.
    Initialised(String),
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
    Input(crate::raw_input::ParsedInput),
    /// The visibility of the end user's cursor.
    CursorVisibility(bool),
    /// Tattoy's configuration.
    Config(crate::config::main::Config),
    /// A known user-defined keybinding event was triggered.
    KeybindEvent(crate::config::input::KeybindingAction),
    /// User notifications in the the UI
    Notification(crate::tattoys::notifications::message::Message),
}

/// Main entrypoint
pub(crate) async fn run(state_arc: &std::sync::Arc<SharedState>) -> Result<()> {
    let protocol_tx = state_arc.protocol_tx.clone();
    let cli_args = setup(state_arc).await?;
    let palette_config_exists =
        crate::palette::parser::Parser::palette_config_exists(state_arc).await;

    if cli_args.capture_palette {
        crate::palette::parser::Parser::run(state_arc, None).await?;
        #[expect(clippy::exit, reason = "We don't want to actually run Tattoy")]
        std::process::exit(0);
    }

    if let Some(screenshot) = cli_args.parse_palette {
        crate::palette::parser::Parser::run(state_arc, Some(&screenshot)).await?;
        #[expect(clippy::exit, reason = "We don't want to actually run Tattoy")]
        std::process::exit(0);
    }

    if !palette_config_exists {
        crate::palette::parser::Parser::run(state_arc, None).await?;
    }

    let users_tty_size = crate::renderer::Renderer::get_users_tty_size()?;
    state_arc
        .set_tty_size(
            users_tty_size.cols.try_into()?,
            users_tty_size.rows.try_into()?,
        )
        .await;

    let (renderer, surfaces_tx) = Renderer::start(Arc::clone(state_arc), protocol_tx.clone());

    let config_handle = crate::config::main::Config::watch(Arc::clone(state_arc));
    let input_thread_handle = RawInput::start(protocol_tx.clone());

    override_on_panic_behaviour();
    let tattoys_handle = crate::loader::start_tattoys(
        cli_args.enabled_tattoys.clone(),
        surfaces_tx.clone(),
        Arc::clone(state_arc),
    );

    let scrollback_size = state_arc.config.read().await.scrollback_size;
    let shadow_terminal_config = shadow_terminal::shadow_terminal::Config {
        width: users_tty_size.cols.try_into()?,
        height: users_tty_size.rows.try_into()?,
        command: get_startup_command(state_arc, cli_args).await?,
        scrollback_size: scrollback_size.try_into()?,
        ..Default::default()
    };
    crate::terminal_proxy::proxy::Proxy::start(
        Arc::clone(state_arc),
        surfaces_tx,
        protocol_tx.clone(),
        shadow_terminal_config,
    )
    .await?;
    tracing::debug!("🏁 left PTY thread, exiting Tattoy...");
    broadcast_protocol_end(&protocol_tx);

    tattoys_handle
        .join()
        .map_err(|err| color_eyre::eyre::eyre!("Tattoys handle: {err:?}"))??;
    if input_thread_handle.is_finished() {
        // The STDIN loop doesn't listen to the global Tattoy protocol, so it can't exit its loop.
        // Therefore we should only join it if it finished due of its own error.
        input_thread_handle
            .join()
            .map_err(|err| color_eyre::eyre::eyre!("STDIN handle: {err:?}"))??;
    }
    renderer.await??;
    config_handle.await??;

    tracing::trace!("Leaving Tattoy's main `run()` function");
    Ok(())
}

/// Block until the given system has ommitted its startup message.
pub(crate) async fn wait_for_system(
    mut protocol: tokio::sync::broadcast::Receiver<Protocol>,
    system: &str,
) {
    tracing::debug!("Waiting for {system} to initialise...");
    loop {
        let Ok(message) = protocol.recv().await else {
            continue;
        };

        if let crate::run::Protocol::Initialised(initialised_system) = message {
            if initialised_system == system {
                break;
            }
        }
    }
    tracing::debug!("...{system} system initialised.");
}

/// The default behaviour prints all panics to the CLI. But we don't want that to happen in
/// tattoy tasks. However `set_hook` globally changes behaviour, therefore it doesn't allow things
/// like only changing behaviour for a block. So we want this to be called as late as posssible so
/// it only affects tattoy tasks. Currently the only main-thread system that we'd want to see
/// panics for, is the Shadow Terminal. At least a log is made. But it would be good to figure out
/// a way to notify developers especially, that the Shadow Terminal panicked.
fn override_on_panic_behaviour() {
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(message) = info.payload().downcast_ref::<String>() {
            message
        } else if let Some(message) = info.payload().downcast_ref::<&str>() {
            message
        } else {
            "Caught a panic with an unknown type."
        };
        let location = match info.location() {
            Some(location) => format!(
                "{}@{}:{}",
                location.file(),
                location.line(),
                location.column()
            ),
            None => "Unknown location".to_owned(),
        };
        tracing::error!("Caught panic ({}): {message:?}", location);
    }));
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

    let mut main_config_file = state.main_config_file.write().await;
    (*main_config_file).clone_from(&cli_args.main_config);
    drop(main_config_file);

    let directory_result =
        crate::config::main::Config::setup_directory(cli_args.config_dir.clone(), state).await;
    if let Err(directory_error) = directory_result {
        color_eyre::eyre::bail!("Error setting up config directory: {directory_error:?}");
    }

    let config_result = crate::config::main::Config::load_config_into_shared_state(state).await;
    if let Err(config_error) = config_result {
        let path = crate::config::main::Config::main_config_path(state).await;
        color_eyre::eyre::bail!(
            "Bad config file: {config_error:?}\n\nConfig path: {}",
            path.display()
        );
    }

    setup_logging(cli_args.clone(), state).await?;

    if cli_args.disable_indicator {
        state.config.write().await.show_tattoy_indicator = false;
    }

    // Assuming true colour makes Tattoy simpler.
    // * I think it's safe to assume that the vast majority of people using Tattoy will have a
    //   true color terminal anyway.
    std::env::set_var("COLORTERM", "truecolor");

    tracing::info!("Starting Tattoy");
    tracing::debug!("Loaded config: {:?}", state.config.read().await);

    let tty_size = crate::renderer::Renderer::get_users_tty_size()?;
    state
        .set_tty_size(tty_size.cols.try_into()?, tty_size.rows.try_into()?)
        .await;

    Ok(cli_args)
}

/// Setup logging
async fn setup_logging(cli_args: CliArgs, state: &std::sync::Arc<SharedState>) -> Result<()> {
    let are_log_filters_manually_set = std::env::var("TATTOY_LOG").is_ok();
    let mut path = state.config.read().await.log_path.clone();

    if let Some(cli_override_path) = cli_args.log_path {
        path = cli_override_path;
    }

    let mut level = state.config.read().await.log_level.clone();
    if let Some(cli_override_level) = cli_args.log_level {
        level = cli_override_level;
    }
    let level_as_string = format!("{level:?}").to_lowercase();

    let is_loggable =
        !matches!(level, crate::config::main::LogLevel::Off) || are_log_filters_manually_set;

    if !is_loggable {
        return Ok(());
    }

    let directory = path.parent().context("Couldn't get log path's parent")?;
    std::fs::create_dir_all(directory)?;
    let file = std::fs::File::create(path)?;

    let filters = if are_log_filters_manually_set {
        if let Ok(user_filters) = std::env::var("TATTOY_LOG") {
            std::env::set_var("RUST_LOG", user_filters);
        }

        // When defining your own filters with `TATTOY_LOG` or `RUST_LOG` set to debug
        // or trace, you'll very likely also want `tokio=debug,runtime=debug`. They're
        // very noisy and most of it is just for the Tokio console, which aren't needed
        // anyway as they're parsed internally.
        tracing_subscriber::EnvFilter::builder()
            .with_default_directive("error".parse()?)
            .from_env_lossy()
    } else {
        tracing_subscriber::EnvFilter::builder()
            .with_default_directive("off".parse()?)
            .from_env_lossy()
            .add_directive(format!("shadow_terminal={level_as_string}").parse()?)
            .add_directive(format!("tattoy={level_as_string}").parse()?)
            .add_directive(format!("tests={level_as_string}").parse()?)
    };

    let logfile_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_filter(filters);

    let tracing_setup = tracing_subscriber::registry().with(logfile_layer);

    if std::env::var_os("ENABLE_TOKIO_CONSOLE") == Some("1".into()) {
        let console_layer = console_subscriber::spawn();
        tracing_setup.with(console_layer).init();
    } else {
        tracing_setup.init();
    }

    let mut is_logging = state.is_logging.write().await;
    *is_logging = true;
    drop(is_logging);

    Ok(())
}

/// Ensure that Tattoy isn't run inside another Tattoy session, unless explicitly desired.
#[expect(
    clippy::print_stderr,
    clippy::exit,
    reason = "This is a valid exit point."
)]
pub fn check_for_tattoy_in_tattoy() {
    let is_running_key = "TATTOY_RUNNING";
    let allow_nested_tattoy = "TATTOY_NEST";
    let is_running = std::env::var(is_running_key).is_ok();
    let is_nesting_allowed = std::env::var(allow_nested_tattoy).unwrap_or_default() == "allow";
    if is_running && !is_nesting_allowed {
        eprintln!(
            "You're trying to run Tattoy inside Tattoy. Whilst this is possible, \
             it can cause issues. If you're sure this is what you want to do then \
             start Tattoy with the environment variable `{allow_nested_tattoy}=allow`."
        );
        std::process::exit(1);
    }

    std::env::set_var(is_running_key, "1");
}
