//! Creates a PTY in an OS subprocess and sends and recieves bytes to/from it over channels.
//! It doesn't actually maintain a visual representation, that requires the [`Wezterm`] terminal
//! to parse the PTY's output, see: [`ShadowTerminal`].

use std::ffi::OsString;
use std::os::fd::FromRawFd as _;

use snafu::{OptionExt as _, ResultExt as _};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::mpsc,
};

/// A single payload from the PTY output stream.
pub type BytesFromPTY = [u8; 4096];
/// A single payload from the user's input stream.
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
    fn setup_pty(
        &self,
    ) -> Result<(tokio::fs::File, portable_pty::PtyPair), crate::errors::PTYError> {
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
        Self::kill_on_protocol_end(self.control_tx.subscribe(), spawn);

        // I originally used the raw file descriptor based on discussions here:
        //   [Github comment](https://github.com/wez/wezterm/discussions/5151)
        // But since then I use it more because `pair.master.try_clone_reader()` doesn't detect the
        // end of the PTY process and so blocks the whole Tattoy app. Listening on the raw FD
        // however, does detect the end of the PTY and so we can exit gracefully.
        let master_fd = pair
            .master
            .as_raw_fd()
            .with_whatever_context(|| "Couldn't get master file descriptor for PTY")?;

        tracing::trace!("Returning PTY file descriptor");
        Ok((
            // SAFETY: Why is this unsafe? Is there another safe way to do this?
            unsafe { tokio::fs::File::from_raw_fd(master_fd) },
            pair,
        ))
    }

    /// Listen for the `End` message from the Tattoy protocol channel and then kill the PTY.
    fn kill_on_protocol_end(
        mut protocol_out: tokio::sync::broadcast::Receiver<crate::Protocol>,
        mut spawn: Box<dyn portable_pty::Child + Send + Sync>,
    ) {
        tokio::spawn(async move {
            tracing::debug!("Starting loop for PTY spawn to receive protocol messages");
            loop {
                match protocol_out.recv().await {
                    Ok(message) => match message {
                        crate::Protocol::End => {
                            tracing::debug!("PTY received Tattoy message {message:?}");
                            let result = spawn.kill();
                            if let Err(error) = result {
                                tracing::error!("Couldn't kill PTY: {error:?}");
                                // TODO: maybe we want to force exit here?
                            }
                            tracing::debug!("PTY sent kill signals");
                            break;
                        }
                        crate::Protocol::Resize { .. } => (),
                    },
                    Err(error) => {
                        tracing::error!("Reading protocol from PTY loop: {error:?}");
                    }
                }
            }
            tracing::debug!("Leaving spawn shutdown listener loop.");
        });
    }

    /// Start the PTY
    pub async fn run(
        self,
        input_rx: mpsc::Receiver<BytesFromSTDIN>,
    ) -> Result<(), crate::errors::PTYError> {
        // It's important that we subscribe now, as that is what starts the backlog of protocol
        // messages. It's possible that messages are sent during PTY startup and we don't want to
        // miss any of those messages later when we finally start the listening loop.
        let mut protocol_for_main_loop = self.control_tx.subscribe();

        let (pty_raw_device_file, pty_pair) = self.setup_pty()?;
        let mut pty_stream_reader = tokio::io::BufReader::new(
            pty_raw_device_file
                .try_clone()
                .await
                .with_whatever_context(|_| "Couldn't clone raw PTY device reader")?,
        );

        // We have to drop the slave so that we don't hang on it when we exit.
        drop(pty_pair.slave);

        let protocol_for_input_loop = self.control_tx.subscribe();
        tokio::spawn(async move {
            let result = Self::forward_input(
                input_rx,
                pty_raw_device_file,
                pty_pair.master,
                protocol_for_input_loop,
            )
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
                result = self.read_stream(&mut pty_stream_reader) => {
                    if let Err(error) = result {
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
                        Err(err) => snafu::whatever!("{err:?}"),

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
        pty_stream_reader: &mut tokio::io::BufReader<tokio::fs::File>,
    ) -> Result<(), crate::errors::PTYError> {
        let mut buffer: BytesFromPTY = [0; 4096];
        let chunk_size = pty_stream_reader.read(&mut buffer[..]).await;
        match chunk_size {
            Ok(0) => {
                snafu::whatever!("PTY reader received 0 bytes");
            }
            Ok(size) => {
                let result = self.output_tx.send(buffer).await;
                if let Err(err) = result {
                    tracing::error!("Sending bytes on PTY output channel: {err}");
                };

                // Debugging only
                // TODO: only do this is dev builds?
                let payload = &buffer
                    .get(0..size)
                    .with_whatever_context(|| "No data in buffer (should be impossible)")?;
                let output = String::from_utf8_lossy(payload)
                    .to_string()
                    .replace('[', "\\[");
                let sample = output
                    .get(0..std::cmp::min(output.len(), 10))
                    .with_whatever_context(|| {
                        "Not enough characters in sample output (should be impossible)"
                    })?;
                tracing::trace!("Sent PTY output ({size}): '{}'...", sample);
            }
            Err(err) => {
                snafu::whatever!("Reading PTY stream: {err}");
            }
        }

        Ok(())
    }

    // Note: I wonder if using `termwiz::terminal::new_terminal`'s' `poll_input()` method is also
    // an option? I think it might not be because we're not actually intercepting `CTRL+C`, `CTRL+D`,
    // etc. But it might be useful for Tattoy-specific keybindings?
    //
    /// Forward channel bytes from the user's input to the virtual PTY
    async fn forward_input(
        mut user_input: mpsc::Receiver<BytesFromSTDIN>,
        pty_stream: tokio::fs::File,
        pty_master: std::boxed::Box<(dyn portable_pty::MasterPty + std::marker::Send + 'static)>,
        mut protocol: tokio::sync::broadcast::Receiver<crate::Protocol>,
    ) -> Result<(), crate::errors::PTYError> {
        tracing::debug!("Starting `forward_input` loop");

        let mut writer = tokio::io::BufWriter::new(pty_stream);

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
                    Self::handle_input_bytes(some_bytes, &mut writer).await?;
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
                tracing::debug!("Resize event received on protocol {message:?}");

                let result = pty_master.resize(Self::pty_size(*width, *height));
                if result.is_err() {
                    tracing::error!("Couldn't resize underlying PTY subprocesss: {result:?}");
                }
            }
            Err(err) => snafu::whatever!("{err:?}"),
        };

        Ok(())
    }

    /// Handle input from end user.
    async fn handle_input_bytes(
        bytes: BytesFromSTDIN,
        pty_stdin: &mut tokio::io::BufWriter<tokio::fs::File>,
    ) -> Result<(), crate::errors::PTYError> {
        tracing::trace!(
            "Forwarding input to PTY: '{}'",
            String::from_utf8_lossy(&bytes).replace('\n', "\\n")
        );

        // TODO:
        // Sending the entire payload seems to break some input 🤔
        // Also, is it more efficient like this? Not sending more bytes than is needed probably
        // prevents some unnecessary parsing somewhere?
        for byte in bytes {
            if byte == 0 {
                break;
            }
            pty_stdin
                .write_all(&[byte])
                .await
                .with_whatever_context(|err| {
                    format!("Couldn't write bytes into PTY's STDIN: {err:?}")
                })?;
        }
        pty_stdin
            .flush()
            .await
            .with_whatever_context(|err| format!("Couldn't flush stdin stream to PTY: {err:?}"))?;

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

        let (pty_output_tx, mut pty_output_rx) = mpsc::channel::<BytesFromPTY>(1);
        let (pty_input_tx, pty_input_rx) = mpsc::channel::<BytesFromSTDIN>(1);
        let (protocol_tx, _) = tokio::sync::broadcast::channel(16);

        let output_task = tokio::spawn(async move {
            tracing::debug!("TEST: Output listener loop starting...");
            let mut result: Vec<u8> = vec![];
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
            let result = pty.run(pty_input_rx).await;
            if let Err(err) = result {
                tracing::warn!("PTY (for tests) handle: {err:?}");
            }
            tracing::debug!("Test PTY.run() done");
        });

        tracing::debug!("TEST: Leaving run helper...");
        (output_task, pty_input_tx)
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

    #[tokio::test]
    async fn rendering_pi() {
        let (output_task, _input_channel) = run(vec![
            "bash".into(),
            "-c".into(),
            "echo 'scale=10; 4*a(1)' | bc -l".into(),
        ]);
        let result = output_task.await.unwrap();
        assert!(result.contains("3.1415926532"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn interactive() {
        let (output_task, input_channel) = run(vec!["bash".into()]);
        input_channel
            .send(stdin_bytes("echo Hello && exit\n"))
            .await
            .unwrap();
        let result = output_task.await.unwrap();
        assert!(result.contains("Hello"));
    }
}
