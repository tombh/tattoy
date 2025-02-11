//! A steppable terminal, useful for doing end to end testing of TUI applications.

use std::fmt::Write as _;
use std::io::Read as _;
use std::sync::Arc;

use snafu::ResultExt as _;
use tracing::Instrument as _;

/// The default time to wait looking for terminal screen content.
const DEFAULT_TIMEOUT: u32 = 500;

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

        let current_span = tracing::Span::current();
        let pty_handle_arc = Arc::clone(&self.pty_task_handle);
        let tokio_runtime = tokio::runtime::Handle::current();
        let result = std::thread::spawn(move || {
            tokio_runtime.block_on(
                async {
                    tracing::trace!("Starting manual loop to wait for PTY task handle to finish");
                    let pty_handle = pty_handle_arc.lock().await;
                    for i in 0i64..=100 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                        if i == 100 {
                            tracing::error!(
                                "Couldn't leave ShadowTerminal handle in 100 iterations"
                            );
                            break;
                        }
                        if pty_handle.is_finished() {
                            tracing::trace!("`pty_handle.finished()` returned `true`");
                            break;
                        }
                    }
                }
                .instrument(current_span),
            );
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
            buffer = [0; 128];
        }

        Ok(())
    }

    /// The same as `send_string()` but just appends a new line, to execute the string as a
    /// command. This will only work if the terminal is currently in a interactive REPL.
    ///
    /// # Errors
    /// If sending the string fails
    #[inline]
    pub fn send_command(&self, command: &str) -> Result<(), crate::errors::PTYError> {
        let string = format!("{command}\n");
        self.send_string(string.as_str())?;

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

    /// Get the position of the top of the screen in the scrollback history.
    ///
    /// # Errors
    /// If it can't convert the position from `isize` to `usize`
    #[inline]
    pub fn get_scrollback_position(
        &mut self,
    ) -> Result<usize, crate::errors::SteppableTerminalError> {
        let screen = self.shadow_terminal.terminal.screen();
        let scrollback_position: usize = screen
            .phys_to_stable_row_index(0)
            .try_into()
            .with_whatever_context(|err| format!("Couldn't scrollback position to usize: {err}"))?;

        Ok(scrollback_position)
    }

    /// Convert the current Wezterm shadow terminal screen to a plain string.
    ///
    /// # Errors
    /// If it can't write into the output string
    #[inline]
    pub fn screen_as_string(&mut self) -> Result<String, crate::errors::SteppableTerminalError> {
        let size = self.shadow_terminal.terminal.get_size();
        let mut screen = self.shadow_terminal.terminal.screen().clone();
        let mut output = String::new();
        let scrollback = self.get_scrollback_position()?;
        for y in scrollback..size.rows {
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

    /// Return the screen coordinates of a matching cell's contents.
    ///
    /// # Errors
    /// If it can't write into the output string
    #[inline]
    pub fn get_coords_of_cell_by_content(&mut self, content: &str) -> Option<(usize, usize)> {
        let size = self.shadow_terminal.terminal.get_size();
        let mut screen = self.shadow_terminal.terminal.screen().clone();
        for y_usize in 0..size.rows {
            let result = y_usize.try_into();

            #[expect(
                clippy::unreachable,
                reason = "I assume that get_size() wouldn't return anything thet get_cell can't consume"
            )]
            let Ok(y) = result
            else {
                unreachable!()
            };
            for x in 0..size.cols {
                let maybe_cell = screen.get_cell(x, y);
                if let Some(cell) = maybe_cell {
                    if cell.str() == content {
                        return Some((x, y_usize));
                    }
                }
            }
        }

        None
    }

    /// Get the [`wezterm_term::Cell`] at the given coordinates.
    ///
    /// # Errors
    /// If the cell at the given coordinates cannot be fetched.
    #[inline]
    pub fn get_cell_at(
        &mut self,
        x: usize,
        y: usize,
    ) -> Result<Option<wezterm_term::Cell>, crate::errors::SteppableTerminalError> {
        let size = self.shadow_terminal.terminal.get_size();
        let mut screen = self.shadow_terminal.terminal.screen().clone();
        let scrollback = self.get_scrollback_position()?;
        for row in scrollback..size.rows {
            for col in 0..size.cols {
                if !(x == col && y == row - scrollback) {
                    continue;
                }

                let maybe_cell = screen.get_cell(
                    col,
                    row.try_into().with_whatever_context(|err| {
                        format!("Couldn't convert cell index to i64: {err}")
                    })?,
                );

                if let Some(cell) = maybe_cell {
                    return Ok(Some(cell.clone()));
                }
            }
        }

        Ok(None)
    }

    /// Get the string, of the given length, at the given coordinates.
    ///
    /// # Errors
    /// If any of the cells at the given coordinates cannot be fetched.
    #[inline]
    pub fn get_string_at(
        &mut self,
        x: usize,
        y: usize,
        length: usize,
    ) -> Result<String, crate::errors::SteppableTerminalError> {
        let mut string = String::new();
        for col in x..(x + length) {
            let maybe_cell = self.get_cell_at(col, y)?;
            if let Some(cell) = maybe_cell {
                string = format!("{string}{}", cell.str());
            }
        }

        Ok(string)
    }

    /// Prints the contents of the current screen to STDERR
    ///
    /// # Errors
    /// If it can't get the screen output.
    #[expect(clippy::print_stderr, reason = "This is a debugging function")]
    #[inline]
    pub fn dump_screen(&mut self) -> Result<(), crate::errors::SteppableTerminalError> {
        let size = self.shadow_terminal.terminal.get_size();
        let current_screen = self.screen_as_string()?;
        eprintln!("Current Tattoy screen ({}x{})", size.cols, size.rows);
        eprintln!("{current_screen}");
        Ok(())
    }

    /// Get the prompt as a string. Useful for reproducibility as prompts can change between
    /// machines.
    ///
    /// # Errors
    /// * If a steppable terminal can't be created.
    /// * If the terminal's screen can't be parsed.
    #[tracing::instrument(name = "get_prompt")]
    #[inline]
    pub async fn get_prompt_string(
        shell: std::ffi::OsString,
    ) -> Result<String, crate::errors::SteppableTerminalError> {
        tracing::info!("Starting `get_prompt` terminal instance...");
        let config = crate::shadow_terminal::Config {
            width: 30,
            height: 10,
            command: vec![shell],
            ..crate::shadow_terminal::Config::default()
        };
        let mut stepper = Self::start(config).await?;
        let mut output = stepper.screen_as_string()?;
        tracing::info!("Finished `get_prompt` terminal instance.");

        output.retain(|character| !character.is_whitespace());
        Ok(output)
    }

    // TODO: Make the timeout configurable.
    //
    /// Wait for the screen to change in any way.
    ///
    /// # Errors
    /// * If it can't get the screen contents.
    /// * If no change is found within a certain time.
    #[inline]
    pub async fn wait_for_any_change(
        &mut self,
    ) -> Result<(), crate::errors::SteppableTerminalError> {
        let initial_screen = self.screen_as_string()?;
        for i in 0..=DEFAULT_TIMEOUT {
            if i == DEFAULT_TIMEOUT {
                snafu::whatever!("No change detected in {DEFAULT_TIMEOUT} milliseconds.");
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

    /// Wait for the given string to appear anywhere in the screen.
    ///
    /// # Errors
    /// * If it can't get the screen contents.
    /// * If no change is found within a certain time.
    #[inline]
    pub async fn wait_for_string(
        &mut self,
        string: &str,
        maybe_timeout: Option<u32>,
    ) -> Result<(), crate::errors::SteppableTerminalError> {
        let timeout = maybe_timeout.map_or(DEFAULT_TIMEOUT, |ms| ms);

        for i in 0u32..=timeout {
            self.render_all_output();
            let current_screen = self.screen_as_string()?;
            if current_screen.contains(string) {
                break;
            }
            if i == timeout {
                self.dump_screen()?;
                snafu::whatever!("'{string}' not found after {timeout} milliseconds.");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }

        Ok(())
    }

    /// Wait for the given string to appear at the given coordinates.
    ///
    /// # Errors
    /// * If it can't get the screen contents.
    /// * If no change is found within a certain time.
    #[inline]
    pub async fn wait_for_string_at(
        &mut self,
        string_to_find: &str,
        x: usize,
        y: usize,
        maybe_timeout: Option<u32>,
    ) -> Result<(), crate::errors::SteppableTerminalError> {
        let timeout = maybe_timeout.map_or(DEFAULT_TIMEOUT, |ms| ms);

        for i in 0u32..=timeout {
            self.render_all_output();
            let found_string = self.get_string_at(x, y, string_to_find.len())?;
            if found_string == string_to_find {
                break;
            }
            if i == timeout {
                self.dump_screen()?;
                snafu::whatever!(
                    "'{string_to_find}' not found at {x}x{y} after {timeout} milliseconds."
                );
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

    async fn run() -> SteppableTerminal {
        let config = Config {
            width: 50,
            height: 10,
            command: vec!["sh".into()],
            ..Config::default()
        };
        SteppableTerminal::start(config).await.unwrap()
    }

    fn setup_logging() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .without_time()
            .init();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn basic_interactivity() {
        let prompt = SteppableTerminal::get_prompt_string("sh".into())
            .await
            .unwrap();
        let mut stepper = run().await;

        let question = "echo $((2*3*3*5*3607*3803))";
        let answer = "1234567890";
        stepper.send_command(question).unwrap();
        stepper.wait_for_string(answer, None).await.unwrap();
        let output = stepper.screen_as_string().unwrap();
        assert_eq!(
            output,
            indoc::formatdoc! {"
                {prompt} {question}
                {answer}
                {prompt} 
                \n\n\n\n\n\n
            "}
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resizing() {
        setup_logging();
        let mut stepper = run().await;
        stepper.send_command("nano --restricted").unwrap();
        stepper.wait_for_string("GNU nano", None).await.unwrap();

        let size = stepper.shadow_terminal.terminal.get_size();
        let bottom = size.rows - 1;
        let right = size.cols - 1;
        let menu_item_paste = stepper.get_string_at(right - 10, bottom, 5).unwrap();
        assert_eq!(menu_item_paste, "Paste");

        stepper
            .shadow_terminal
            .resize(
                u16::try_from(size.cols + 3).unwrap(),
                u16::try_from(size.rows + 3).unwrap(),
            )
            .unwrap();
        let resized_size = stepper.shadow_terminal.terminal.get_size();
        let resized_bottom = resized_size.rows - 1;
        let resized_right = resized_size.cols - 1;
        stepper
            .wait_for_string_at("^X Exit", 0, resized_bottom, Some(500))
            .await
            .unwrap();
        let resized_menu_item_paste = stepper
            .get_string_at(resized_right - 10, resized_bottom, 5)
            .unwrap();
        assert_eq!(resized_menu_item_paste, "Paste");
    }
}
