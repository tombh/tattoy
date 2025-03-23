//! Creates a PTY in an OS subprocess and sends and recieves bytes to/from it over channels.
//! It doesn't actually maintain a visual representation, that requires the [`Wezterm`] terminal
//! to parse the PTY's output, see: [`ShadowTerminal`].

use std::{ffi::OsString, io::Read as _};

use snafu::{OptionExt as _, ResultExt as _};
use tokio::sync::mpsc;
use tracing::Instrument as _;

/// A single payload from the PTY output stream.
pub type BytesFromPTY = [u8; 4096];
/// A single payload from the user's input stream (or sometimes internal input).
pub type BytesFromSTDIN = [u8; 128];

/// This is the PTY process that replaces the user's current TTY
#[non_exhaustive]
pub struct PTY {
    /// PTY starting command
    pub command: Vec<OsString>,
    /// PTY width
    pub width: u16,
    /// PTY height
    pub height: u16,
    /// Send side of channel to send control messages like; shutdown and resize.
    pub control_tx: tokio::sync::broadcast::Sender<crate::Protocol>,
    /// Send side of channel sending updates from the PTY process
    pub output_tx: tokio::sync::mpsc::Sender<crate::pty::BytesFromPTY>,
}

impl PTY {
    /// Function just to isolate the PTY setup
    fn setup_pty(&self) -> Result<portable_pty::PtyPair, crate::errors::PTYError> {
        tracing::debug!("Setting up PTY");
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(Self::pty_size(self.width, self.height))
            .with_whatever_context(|_| "Error opening PTY")?;

        tracing::debug!("Launching `{:?}` on PTY", self.command);
        let mut cmd = portable_pty::CommandBuilder::from_argv(self.command.clone());
        cmd.cwd(
            std::env::current_dir()
                .with_whatever_context(|_| "Couldn't get user's current directory")?,
        );
        let spawn = pair
            .slave
            .spawn_command(cmd)
            .with_whatever_context(|_| "Error spawning PTY command")?;
        let killer = spawn.clone_killer();
        Self::wait_for_pty_end(self.control_tx.clone(), spawn);
        Self::kill_on_protocol_end(self.control_tx.subscribe(), killer);

        tracing::trace!("Returning PTY pair");
        Ok(pair)
    }

    /// The PTY crate is not async, so here we're basically just listening to the PTY to be able to
    /// broadcastr it's output on an async channel.
    fn pty_reader_loop(
        pty_reader: std::boxed::Box<dyn std::io::Read + std::marker::Send>,
        pty_reader_tx: mpsc::Sender<BytesFromPTY>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_blocking(move || {
            let mut reader = std::io::BufReader::new(pty_reader);
            loop {
                let mut buffer: BytesFromPTY = [0; 4096];
                let read_result = reader.read(&mut buffer);
                match read_result {
                    Ok(0) => {
                        tracing::debug!("PTY reader loop received 0 bytes, exiting...");
                        break;
                    }
                    Ok(_) => {
                        let send_result = pty_reader_tx.blocking_send(buffer);
                        if let Err(error) = send_result {
                            tracing::error!("Broadcasting PTY output: {error:?}");
                            break;
                        }
                    }
                    Err(error) => tracing::error!("PTY reader: {error:?}"),
                }
            }
            tracing::trace!("Leaving PTY reader loop");
        })
    }

    /// A dedicated loop to listen for the official PTY end event.
    fn wait_for_pty_end(
        protocol_out: tokio::sync::broadcast::Sender<crate::Protocol>,
        mut spawn: Box<dyn portable_pty::Child + Send + Sync>,
    ) {
        tokio::task::spawn_blocking(move || {
            tracing::debug!("Starting to wait for PTY end");
            let waiter_result = spawn.wait();
            if let Err(error) = waiter_result {
                tracing::error!("Waiting for PTY: {error:?}");
            }
            let sender_result = protocol_out.send(crate::Protocol::End);
            if let Err(error) = sender_result {
                tracing::error!("Sending `Protocol::End` after: {error:?} ");
            }
            tracing::info!("PTY ended by its own accord");
        });
    }

    /// Listen for the `End` message from the Tattoy protocol channel and then kill the PTY.
    fn kill_on_protocol_end(
        mut protocol_in: tokio::sync::broadcast::Receiver<crate::Protocol>,
        mut spawn: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    ) {
        let current_span = tracing::Span::current();
        tokio::spawn(
            async move {
                tracing::debug!("Starting loop for PTY spawn to receive protocol messages");
                loop {
                    match protocol_in.recv().await {
                        Ok(message) => {
                            if matches!(message, crate::Protocol::End)  {
                                tracing::debug!("PTY received Tattoy message {message:?}");
                                let result = spawn.kill();
                                if let Err(error) = result {
                                    // This is the error when the PTY naturally ends. Is there a better way to
                                    // match?
                                    let pty_exit = "No such process";
                                    if error.to_string().contains(pty_exit) {
                                        tracing::debug!("Tried killing PTY that was already gone.");
                                        break;
                                    }

                                    tracing::error!("Couldn't kill PTY: {error:?}");
                                    // TODO: maybe we want to force exit here?
                                }

                                tracing::debug!(
                                    "`kill()` (which includes OS kill signals) sent to PTY spawn process"
                                );
                                break;
                            }
                        }
                        Err(error) => {
                            tracing::error!("Reading protocol from PTY loop: {error:?}");
                        }
                    }
                }
                tracing::debug!("Leaving spawn shutdown listener loop.");
            }
            .instrument(current_span),
        );
    }

    /// Start the PTY
    pub async fn run(
        self,
        user_input_rx: mpsc::Receiver<BytesFromSTDIN>,
        internal_input_rx: mpsc::Receiver<BytesFromSTDIN>,
    ) -> Result<(), crate::errors::PTYError> {
        let (pty_reader_tx, mut pty_reader_rx) = tokio::sync::mpsc::channel(1);

        // It's important that we subscribe now, as that is what starts the backlog of protocol
        // messages. It's possible that messages are sent during PTY startup and we don't want to
        // miss any of those messages later when we finally start the listening loop.
        let mut protocol_for_main_loop = self.control_tx.subscribe();

        let pty_pair = self.setup_pty()?;
        let pty_writer = pty_pair
            .master
            .take_writer()
            .with_whatever_context(|err| format!("Getting PTY writer: {err:?}"))?;
        let pty_reader = pty_pair
            .master
            .try_clone_reader()
            .with_whatever_context(|err| format!("Getting PTY reader: {err:?}"))?;

        Self::pty_reader_loop(pty_reader, pty_reader_tx);

        // We have to drop the slave so that we don't hang on it when we exit.
        drop(pty_pair.slave);

        // TODO: should we be handling any errors in here?
        let protocol_for_input_loop = self.control_tx.subscribe();
        let current_span = tracing::Span::current();
        tokio::spawn(async move {
            let result = Self::forward_input(
                user_input_rx,
                internal_input_rx,
                pty_writer,
                pty_pair.master,
                protocol_for_input_loop,
            )
            .instrument(current_span)
            .await;
            if let Err(err) = result {
                tracing::error!("Writing to PTY stream: {err}");
            }
        });

        tracing::debug!("Starting PTY reader loop");
        #[expect(
            clippy::integer_division_remainder_used,
            reason = "`tokio::select! generates this.`"
        )]
        loop {
            tokio::select! {
                result = self.read_stream(&mut pty_reader_rx) => {
                    if let Err(error) = result {
                        // TODO: The error should be bubbled, and logged centrally
                        tracing::error!("{error:?}");
                        snafu::whatever!("{error:?}");
                    }
                }
                result = protocol_for_main_loop.recv() => {
                    match result {
                        Ok(message) => {
                            if matches!(message, crate::Protocol::End) {
                                break;
                            }
                        }
                        Err(err) => {
                            // TODO: The error should be bubbled, and logged centrally
                            tracing::error!("{err:?}");
                            snafu::whatever!("{err:?}");
                        },

                    }
                }

            }
        }

        tracing::debug!("PTY reader loop finished");
        Ok(())
    }

    /// Read bytes from the underlying PTY sub process and forward them to the Shadow Terminal.
    async fn read_stream(
        &self,
        pty_reader_rx: &mut mpsc::Receiver<BytesFromPTY>,
    ) -> Result<(), crate::errors::PTYError> {
        let Some(bytes) = pty_reader_rx.recv().await else {
            return Ok(());
        };

        let result = self.output_tx.send(bytes).await;
        if let Err(err) = result {
            tracing::error!("Sending bytes on PTY output channel: {err}");
        }

        let output = String::from_utf8_lossy(&bytes)
            .to_string()
            .replace('\x1b', "^");
        tracing::trace!("Sent PTY output, sample:\n{:.500}...", output);

        Ok(())
    }

    /// Forward channel bytes from the user's input to the virtual PTY
    async fn forward_input(
        mut user_input: mpsc::Receiver<BytesFromSTDIN>,
        mut internal_input: mpsc::Receiver<BytesFromSTDIN>,
        mut pty_writer: std::boxed::Box<dyn std::io::Write + std::marker::Send>,
        pty_master: std::boxed::Box<(dyn portable_pty::MasterPty + std::marker::Send + 'static)>,
        mut protocol: tokio::sync::broadcast::Receiver<crate::Protocol>,
    ) -> Result<(), crate::errors::PTYError> {
        tracing::debug!("Starting `forward_input` loop");

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is generated by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                message = protocol.recv() => {
                    Self::handle_protocol_message_for_input_loop(&message, &pty_master)?;
                    if matches!(message, Ok(crate::Protocol::End)) {
                        break;
                    }
                }
                Some(some_bytes) = user_input.recv() => {
                    Self::handle_input_bytes(some_bytes, &mut pty_writer)?;
                }
                Some(some_bytes) = internal_input.recv() => {
                    Self::handle_input_bytes(some_bytes, &mut pty_writer)?;
                }
            }
        }

        tracing::debug!("`forward_input` loop finished");
        Ok(())
    }

    /// Handle a message from the Tattoy protocol broadcast channel.
    fn handle_protocol_message_for_input_loop(
        message: &std::result::Result<crate::Protocol, tokio::sync::broadcast::error::RecvError>,
        pty_master: &std::boxed::Box<(dyn portable_pty::MasterPty + std::marker::Send + 'static)>,
    ) -> Result<(), crate::errors::PTYError> {
        match message {
            Ok(crate::Protocol::End) => {
                tracing::trace!("PTY input forwarder task received {message:?}");
                return Ok(());
            }
            Ok(crate::Protocol::Resize { width, height }) => {
                tracing::debug!("Resize event received on PTY input loop {message:?}");

                let result = pty_master.resize(Self::pty_size(*width, *height));
                if result.is_err() {
                    tracing::error!("Couldn't resize underlying PTY subprocesss: {result:?}");
                }
            }
            Ok(_) => (),
            Err(err) => snafu::whatever!("{err:?}"),
        }

        Ok(())
    }

    /// Handle input from end user.
    fn handle_input_bytes(
        bytes: BytesFromSTDIN,
        pty_stdin: &mut std::boxed::Box<dyn std::io::Write + std::marker::Send>,
    ) -> Result<(), crate::errors::PTYError> {
        tracing::trace!(
            "Forwarding input to PTY: '{}'",
            String::from_utf8_lossy(&bytes).replace('\n', "\\n")
        );

        let maybe_size = bytes.iter().position(|byte| byte == &0);
        let size = maybe_size.unwrap_or(128);
        let byte_slice = bytes.get(0..size).with_whatever_context(|| {
            "Couldn't get slice of input payload. Should be impossible."
        })?;

        pty_stdin
            .write_all(byte_slice)
            .with_whatever_context(|err| {
                format!("Couldn't write bytes into PTY's STDIN: {err:?}")
            })?;
        pty_stdin
            .flush()
            .with_whatever_context(|err| format!("Couldn't flush STDIN stream to PTY: {err:?}"))?;

        Ok(())
    }

    /// Just a little central place to build the `PtySize` struct consistently.
    const fn pty_size(width: u16, height: u16) -> portable_pty::PtySize {
        portable_pty::PtySize {
            cols: width,
            rows: height,
            // Not all systems support pixel_width, pixel_height,
            // but it is good practice to set it to something
            // that matches the size of the selected font.
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// Add bytes to the beginning
    pub fn add_bytes_to_buffer(
        buffer: &mut BytesFromSTDIN,
        bytes: &[u8],
    ) -> Result<(), crate::errors::PTYError> {
        if bytes.len() > buffer.len() {
            snafu::whatever!(
                "Bytes ({}) to add to buffer are more than the buffer size ({}).",
                bytes.len(),
                buffer.len()
            );
        }
        for (i, chunk_byte) in bytes.iter().enumerate() {
            let buffer_byte = buffer
                .get_mut(i)
                .with_whatever_context(|| "Couldn't get byte from buffer")?;
            *buffer_byte = *chunk_byte;
        }

        Ok(())
    }
}

impl Drop for PTY {
    fn drop(&mut self) {
        tracing::debug!("PTY dropped, broadcasting `End` signal.");

        let result: Result<_, crate::errors::PTYError> = self
            .control_tx
            .send(crate::Protocol::End)
            .with_whatever_context(|err| {
                format!("Couldn't send shutdown signal after PTY finished: {err:?}")
            });

        if let Err(err) = result {
            tracing::error!("{err:?}");
        }
    }
}

#[cfg(test)]
#[expect(clippy::print_stderr, reason = "Tests aren't so strict")]
mod test {
    use super::*;

    fn run(
        command: Vec<OsString>,
    ) -> (
        tokio::task::JoinHandle<std::string::String>,
        mpsc::Sender<BytesFromSTDIN>,
    ) {
        // TODO: Think about a convenient way to enable this whenever only a single test is ran
        // setup_logging().unwrap();

        let (pty_output_tx, mut pty_output_rx) = mpsc::channel::<BytesFromPTY>(8);
        let (pty_input_tx, pty_input_rx) = mpsc::channel::<BytesFromSTDIN>(1);
        let (_, internal_input_rx) = mpsc::channel::<BytesFromSTDIN>(8);
        let (protocol_tx, _) = tokio::sync::broadcast::channel(16);

        let output_task = tokio::spawn(async move {
            tracing::debug!("TEST: Output listener loop starting...");
            let mut result: Vec<u8> = vec![];

            // TODO: don't just rely on test commands sending an `exit` to allow this loop to
            // finish.
            while let Some(bytes) = pty_output_rx.recv().await {
                result.extend(bytes.iter().copied());
            }

            let output = String::from_utf8_lossy(&result).into_owned();
            tracing::debug!("TEST: `interactive()` output: {output:?}");
            output
        });

        tokio::spawn(async move {
            tracing::debug!("TEST: PTY.run() starting...");
            let pty = PTY {
                command,
                width: 10,
                height: 10,
                output_tx: pty_output_tx,
                control_tx: protocol_tx.clone(),
            };
            let result = pty.run(pty_input_rx, internal_input_rx).await;
            if let Err(err) = result {
                tracing::warn!("PTY (for tests) handle: {err:?}");
            }
            tracing::debug!("Test PTY.run() done");
        });

        tracing::debug!("TEST: Leaving run helper...");
        (output_task, pty_input_tx)
    }

    /// TODO: Powershell isn't displaying emoji: 🌍
    fn cat_earth_command() -> String {
        let cat_command = "cat";
        let path = crate::tests::helpers::workspace_dir()
            .join("crates")
            .join("shadow_terminal")
            .join("src")
            .join("tests")
            .join("cat_me.txt");

        #[cfg(not(target_os = "windows"))]
        let sleep = "&& sleep 0.5";
        #[cfg(target_os = "windows")]
        let sleep = "; Start-Sleep -Milliseconds 5";

        format!("{cat_command} {} {sleep}", path.display())
    }

    fn stdin_bytes(input: &str) -> BytesFromSTDIN {
        let mut buffer: BytesFromSTDIN = [0; 128];
        #[expect(
            clippy::indexing_slicing,
            reason = "How do I do a range slice with []?"
        )]
        buffer[..input.len()].copy_from_slice(input.as_bytes());
        buffer
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn basic_output() {
        let mut command = crate::steppable_terminal::get_canonical_shell();

        #[cfg(not(target_os = "windows"))]
        command.push("-c".into());
        #[cfg(target_os = "windows")]
        command.push("-Command".into());

        command.push(cat_earth_command().into());

        let (output_task, _) = run(command);
        let result = output_task.await.unwrap();
        eprintln!("{result}");

        assert!(result.contains("earth"));
    }

    #[cfg(not(target_os = "windows"))]
    #[tokio::test(flavor = "multi_thread")]
    async fn interactive() {
        let (output_task, input_channel) = run(crate::steppable_terminal::get_canonical_shell());
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        #[cfg(not(target_os = "windows"))]
        let exit = "&& exit";
        #[cfg(target_os = "windows")]
        let exit = "; exit";
        let command = format!("{} {exit}\n", cat_earth_command());

        input_channel
            .send(stdin_bytes(command.as_ref()))
            .await
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        let result = output_task.await.unwrap();
        eprintln!("{result}");

        assert!(result.contains("earth"));
    }
}
