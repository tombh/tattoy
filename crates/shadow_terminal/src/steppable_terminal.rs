//! A steppable terminal, useful for doing end to end testing of TUI applications.

use std::fmt::Write as _;
use std::io::Read as _;
use std::sync::Arc;

use snafu::ResultExt as _;

/// This Steppable Terminal is likely more useful for running end to end tests.
///
/// It doesn't run [`ShadowTerminal`] in a loop and so requires calling certain methods manually to advance the
/// terminal frontend. It also exposes the underyling [`Wezterm`] terminal that has a wealth of useful methods
/// for interacting with it.
#[non_exhaustive]
pub struct SteppableTerminal {
    /// The [`ShadowTerminal`] frontend combines a PTY process and a [`Wezterm`] terminal instance.
    pub shadow_terminal: crate::shadow_terminal::ShadowTerminal,
    /// The underlying PTY's Tokio task handle.
    pub pty_task_handle: std::sync::Arc<
        tokio::sync::Mutex<tokio::task::JoinHandle<Result<(), crate::errors::PTYError>>>,
    >,
    /// A Tokio channel that forwards bytes to the underlying PTY's STDIN.
    pub pty_input_tx: tokio::sync::mpsc::Sender<crate::pty::BytesFromSTDIN>,
}

impl SteppableTerminal {
    /// Starts the terminal. Waits for first output before returning.
    ///
    /// # Errors
    /// If it doesn't receive any output in time.
    #[inline]
    pub async fn start(
        config: crate::shadow_terminal::Config,
    ) -> Result<Self, crate::errors::SteppableTerminalError> {
        let shadow_terminal = crate::shadow_terminal::ShadowTerminal::new(config);

        let (pty_input_tx, pty_input_rx) = tokio::sync::mpsc::channel(10);
        let pty_task_handle = shadow_terminal.start(pty_input_rx);

        let mut steppable = Self {
            shadow_terminal,
            pty_task_handle: std::sync::Arc::new(tokio::sync::Mutex::new(pty_task_handle)),
            pty_input_tx,
        };

        for i in 0i8..=100 {
            if i == 100 {
                snafu::whatever!("Shadow Terminal didn't start in time.");
            }
            steppable.render_all_output();
            let mut screen = steppable.screen_as_string()?;
            screen.retain(|character| !character.is_whitespace());
            if !screen.is_empty() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }

        Ok(steppable)
    }

    /// Broadcast the shutdown signal. This should exit both the underlying PTY process and the
    /// main `ShadowTerminal` loop.
    ///
    /// # Errors
    /// If the `End` messaage could not be sent.
    #[inline]
    pub fn kill(&self) -> Result<(), crate::errors::SteppableTerminalError> {
        tracing::info!("Killing Steppable Terminal...");
        self.shadow_terminal.kill().with_whatever_context(|err| {
            format!("Couldn't call `ShadowTerminal.kill()` from SteppableTerminal: {err:?}")
        })?;

        let pty_handle_arc = Arc::clone(&self.pty_task_handle);
        let tokio_runtime = tokio::runtime::Handle::current();
        let result = std::thread::spawn(move || {
            tokio_runtime.block_on(async {
                let pty_handle = pty_handle_arc.lock().await;
                for i in 0i8..100 {
                    if i == 100 {
                        tracing::error!("Couldn't leave ShadowTerminal handle in 100 iterations");
                    }
                    if pty_handle.is_finished() {
                        break;
                    }
                }
            });
        })
        .join();
        if let Err(error) = result {
            snafu::whatever!("Error in thread that spawns PTY handle waiter: {error:?}");
        }

        Ok(())
    }

    /// Send string input directly into the underlying PTY process. This doesn't go through the
    /// shadow terminal's "frontend".
    ///
    /// # Errors
    /// If sending the string fails
    #[inline]
    pub fn send_string(&self, string: &str) -> Result<(), crate::errors::PTYError> {
        let mut reader = std::io::BufReader::new(string.as_bytes());
        let mut buffer: crate::pty::BytesFromSTDIN = [0; 128];
        while let Ok(n) = reader.read(&mut buffer[..]) {
            if n == 0 {
                break;
            }
            self.pty_input_tx
                .try_send(buffer)
                .with_whatever_context(|err| format!("Couldn't send string ({string}): {err:?}"))?;
        }

        Ok(())
    }

    /// Consume all the new output from the underlying PTY and have Wezterm render it in the shadow
    /// terminal.
    ///
    /// Warning: this function could block if there is no end to the output from the PTY.
    #[inline]
    pub fn render_all_output(&mut self) {
        loop {
            let result = self.shadow_terminal.channels.output_rx.try_recv();
            match result {
                Ok(bytes) => {
                    self.shadow_terminal.terminal.advance_bytes(bytes);
                    tracing::trace!("Wezterm shadow terminal advanced {} bytes", bytes.len());
                }
                Err(_) => break,
            }
        }
    }

    /// Convert the current Wezterm shadow terminal screen to a plain string.
    ///
    /// # Errors
    /// If it can write into the output string
    #[inline]
    pub fn screen_as_string(&mut self) -> Result<String, crate::errors::SteppableTerminalError> {
        let size = self.shadow_terminal.terminal.get_size();
        let screen = self.shadow_terminal.terminal.screen_mut();
        let mut output = String::new();
        for y in 0..size.rows {
            for x in 0..size.cols {
                let maybe_cell = screen.get_cell(
                    x,
                    y.try_into().with_whatever_context(|err| {
                        format!("Couldn't convert cell index to i64: {err}")
                    })?,
                );
                if let Some(cell) = maybe_cell {
                    write!(output, "{}", cell.str())
                        .with_whatever_context(|_| "Couldn't write screen output")?;
                }
            }
            writeln!(output).with_whatever_context(|_| "Couldn't write screen output")?;
        }

        Ok(output)
    }

    // TODO: Make the timeout configurable.
    //
    /// Wait for the screen to change.
    ///
    /// # Errors
    /// * If it can get the screen contents.
    /// * If no change is found within a certain time.
    #[inline]
    pub async fn wait_for_change(&mut self) -> Result<(), crate::errors::SteppableTerminalError> {
        let initial_screen = self.screen_as_string()?;
        for i in 0i8..=100 {
            if i == 100 {
                snafu::whatever!("No change detected in 100 milliseconds.");
            }
            self.render_all_output();
            let current_screen = self.screen_as_string()?;
            if initial_screen != current_screen {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }

        Ok(())
    }
}

impl Drop for SteppableTerminal {
    #[inline]
    fn drop(&mut self) {
        tracing::trace!("Running SteppableTerminal.drop()");
        let result = self.kill();
        if let Err(error) = result {
            tracing::error!("{error:?}");
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::shadow_terminal::Config;

    async fn get_prompt() -> String {
        tracing::info!("Starting `get_prompt` terminal instance...");
        let config = Config {
            width: 30,
            height: 10,
            command: vec!["sh".into()],
            ..Config::default()
        };
        let mut stepper = SteppableTerminal::start(config).await.unwrap();
        let mut output = stepper.screen_as_string().unwrap();
        tracing::info!("Finished `get_prompt` terminal instance.");

        output.retain(|character| !character.is_whitespace());
        output
    }

    fn setup_logging() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .without_time()
            .init();
    }

    // TODO are 2 threads really needed?
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn basic_interactivity() {
        setup_logging();

        let prompt = get_prompt().await;

        let config = Config {
            width: 50,
            height: 3,
            command: vec!["sh".into()],
            ..Config::default()
        };
        let mut stepper = SteppableTerminal::start(config).await.unwrap();

        stepper.send_string("echo $((1+1))\n").unwrap();
        stepper.wait_for_change().await.unwrap();

        let output = stepper.screen_as_string().unwrap();
        assert_eq!(
            output,
            indoc::formatdoc! {"
                {prompt} echo $((1+1))
                2
                {prompt} 
            "}
        );
    }
}
