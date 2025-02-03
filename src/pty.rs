//! Create a PTY and send and recieve bytes to/from it over channels.
//! It doesn't actually maintain a visual representation, it's just the subprocesss of the user's
//! actual OG terminal.

use std::ffi::OsString;
use std::io::Read as _;
use std::os::fd::FromRawFd as _;

use color_eyre::eyre::{eyre, ContextCompat as _, Result};
use tokio::sync::mpsc;

use crate::run::Protocol;

/// A single payload from the PTY output stream.
pub type StreamBytesFromPTY = [u8; 4096];
/// A single payload from the user's input stream.
pub type StreamBytesFromSTDIN = [u8; 128];

/// This is the TTY process that replaces the user's current TTY
#[non_exhaustive]
pub(crate) struct PTY {
    /// PTY height
    pub height: u16,
    /// PTY width
    pub width: u16,
    /// PTY starting command
    pub command: Vec<OsString>,
}

impl PTY {
    /// Instantiate
    pub fn new(command: Vec<OsString>) -> Result<Self> {
        let tty_size = crate::renderer::Renderer::get_users_tty_size()?;

        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "It's just PTY sizes"
        )]
        let pty = Self {
            width: tty_size.cols as u16,
            height: tty_size.rows as u16,
            command,
        };
        Ok(pty)
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

    /// Function just to isolate the PTY setup
    fn setup_pty(
        &self,
        protocol_out: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<(std::fs::File, portable_pty::PtyPair)> {
        tracing::debug!("Setting up PTY");
        let pty_system = portable_pty::native_pty_system();
        let pty_result = pty_system.openpty(Self::pty_size(self.width, self.height));

        let pair = match pty_result {
            Ok(pty_ok) => pty_ok,
            Err(pty_error) => {
                // For some reason the error returned by `openpty` can't be handled
                // by the magic `?` operator. hence why I've manually unpacked it.
                let error = format!("Error opening PTY: {pty_error}");
                return Err(eyre!(error));
            }
        };

        tracing::debug!("Launching `{:?}` on PTY", self.command);
        let mut cmd = portable_pty::CommandBuilder::from_argv(self.command.clone());
        cmd.cwd(std::env::current_dir()?);
        let spawn = match pair.slave.spawn_command(cmd) {
            Ok(pty_ok) => pty_ok,
            Err(pty_error) => {
                // For some reason the error returned by `spawn_command` can't be handled
                // by the magic `?` operator. hence why I've manually unpacked it.
                let error = format!("Error spawning PTY command: {pty_error}");
                return Err(eyre!(error));
            }
        };
        Self::kill_on_protocol_end(protocol_out, spawn);

        // I originally used the raw file descriptor based on discussions here:
        //   [Github comment](https://github.com/wez/wezterm/discussions/5151)
        // But since then I use it more because `pair.master.try_clone_reader()` doesn't detect the
        // end of the PTY process and so blocks the whole Tattoy app. Listening on the raw FD
        // however, does detect the end of the PTY and so we can exit gracefully.
        let Some(master_fd) = pair.master.as_raw_fd() else {
            return Err(eyre!("Couldn't get master file descriptor for PTY"));
        };

        tracing::trace!("Returning PTY file descriptor");
        Ok((
            // SAFETY: Why is this unsafe? Is there another safe way to do this?
            unsafe { std::fs::File::from_raw_fd(master_fd) },
            pair,
        ))
    }

    /// Listen for the `End` message from the Tattoy protocol channel and kill the PTY.
    fn kill_on_protocol_end(
        mut protocol_out: tokio::sync::broadcast::Receiver<Protocol>,
        mut spawn: Box<dyn portable_pty::Child + Send + Sync>,
    ) {
        tokio::spawn(async move {
            loop {
                match protocol_out.recv().await {
                    Ok(message) => match message {
                        Protocol::End => {
                            tracing::debug!("PTY received Tattoy message {message:?}");
                            let result = spawn.kill();
                            if let Err(error) = result {
                                tracing::error!("Couldn't kill PTY: {error:?}");
                                // TODO: maybe we want to force exit here?
                            }
                            tracing::debug!("PTY sent kill signals");
                            break;
                        }
                        // Resize is handled on the user input thread.
                        Protocol::Resize { .. } => (),
                    },
                    Err(error) => {
                        tracing::error!("Reading protocol from PTY loop: {error:?}");
                    }
                }
            }
        });
    }

    /// Start the PTY
    pub async fn run(
        &self,
        user_input: mpsc::Receiver<StreamBytesFromSTDIN>,
        pty_output: mpsc::Sender<StreamBytesFromPTY>,
        protocol_in: tokio::sync::broadcast::Receiver<Protocol>,
        protocol_out: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        let (pty_raw_device_file, pty_pair) = self.setup_pty(protocol_out)?;
        let mut pty_stream_reader = std::io::BufReader::new(pty_raw_device_file.try_clone()?);

        // We have to drop the slave so that we don't hang on it when we exit.
        drop(pty_pair.slave);

        tokio::spawn(async move {
            let result = Self::forward_input(
                user_input,
                pty_raw_device_file,
                pty_pair.master,
                protocol_in,
            )
            .await;
            if let Err(err) = result {
                tracing::error!("Writing to PTY stream: {err}");
            }
        });

        tracing::debug!("Starting PTY reader loop");
        loop {
            let mut buffer: StreamBytesFromPTY = [0; 4096];

            let chunk_size = pty_stream_reader.read(&mut buffer[..]);
            match chunk_size {
                Ok(0) => {
                    tracing::debug!("PTY reader received 0 bytes");
                    break;
                }
                Ok(size) => {
                    let result = pty_output.send(buffer).await;
                    if let Err(err) = result {
                        tracing::error!("Sending bytes on PTY output channel: {err}");
                    };

                    // Debugging only
                    // TODO: only do this is dev builds?
                    let payload = &buffer.get(0..size).context("No data in buffer from PTY")?;
                    let output = String::from_utf8_lossy(payload)
                        .to_string()
                        .replace('[', "\\[");
                    let maybe_sample = output
                        .get(0..std::cmp::min(output.len(), 10))
                        .context("Not enough chars in string");
                    if let Ok(sample) = maybe_sample {
                        tracing::trace!("Sending PTY output ({size}): {}", sample);
                    }
                }
                Err(err) => {
                    tracing::warn!("Reading PTY stream: {err}");
                    break;
                }
            }
        }

        // Required to close whatever loop is listening to the output
        drop(pty_output);

        tracing::debug!("PTY reader loop finished");
        Ok(())
    }

    // Note: I wonder if using `termwiz::terminal::new_terminal`'s' `poll_input()` method is also
    // an option? I think it might not be because we're not actually intercepting `CTRL+C`, `CTRL+D`,
    // etc. But it might be useful for Tattoy-specific keybindings?
    //
    /// Forward channel bytes from the user's STDIN to the virtual PTY
    async fn forward_input(
        mut user_input: mpsc::Receiver<StreamBytesFromSTDIN>,
        pty_stream: std::fs::File,
        pty_master: std::boxed::Box<(dyn portable_pty::MasterPty + std::marker::Send + 'static)>,
        mut protocol: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        tracing::debug!("Starting `forward_input` loop");

        // > BufWriter<W> can improve the speed of programs that make small and repeated write calls to
        // > the same file or network socket.
        let mut writer = std::io::BufWriter::new(pty_stream);

        #[expect(
            clippy::integer_division_remainder_used,
            reason = "This is generated by the `tokio::select!`"
        )]
        loop {
            tokio::select! {
                message = protocol.recv() => {
                    Self::handle_protocol_message(&message, &pty_master)?;
                    if matches!(message, Ok(Protocol::End)) {
                        break;
                    }
                }
                some_bytes = user_input.recv() => { Self::handle_bytes_from_stdin(some_bytes, &mut writer)? }
            }
        }

        tracing::debug!("STDIN forwarder loop finished");
        Ok(())
    }

    /// Handle a message from the Tattoy protocol broadcast channel.
    fn handle_protocol_message(
        message: &std::result::Result<
            crate::run::Protocol,
            tokio::sync::broadcast::error::RecvError,
        >,
        pty_master: &std::boxed::Box<(dyn portable_pty::MasterPty + std::marker::Send + 'static)>,
    ) -> Result<()> {
        match message {
            // TODO: should this be oneshot?
            Ok(Protocol::End) => {
                tracing::trace!("STDIN forwarder task received {message:?}");
                return Ok(());
            }
            Ok(Protocol::Resize { width, height }) => {
                tracing::debug!("Resize event received on protocol {message:?}");

                let result = pty_master.resize(Self::pty_size(*width, *height));
                if result.is_err() {
                    tracing::error!("Couldn't resize underlying PTY subprocesss: {result:?}");
                }
            }
            Err(err) => {
                return Err(color_eyre::eyre::Error::new(err.clone()));
            }
        };

        Ok(())
    }

    /// Handle STDIN from user's actual real terminal.
    fn handle_bytes_from_stdin(
        maybe_bytes: Option<StreamBytesFromSTDIN>,
        mut pty_stdin: impl std::io::Write,
    ) -> Result<()> {
        let Some(bytes) = maybe_bytes else {
            return Ok(());
        };

        // Sending the entire payload (`StreamBytesFromSTDIN`) seems to break some input 🤔
        // Also, is it more efficient like this? Not sending more bytes than is needed probably
        // prevents some unnecessary parsing somewhere?
        for byte in bytes {
            if byte == 0 {
                break;
            }
            pty_stdin.write_all(&[byte])?;
        }
        pty_stdin.flush()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        command: Vec<OsString>,
    ) -> (
        tokio::task::JoinHandle<std::string::String>,
        mpsc::Sender<StreamBytesFromSTDIN>,
    ) {
        // TODO: Think about a convenient way to enable this whenever only a single test is ran
        // setup_logging().unwrap();

        let (pty_output_tx, mut pty_output_rx) = mpsc::channel::<StreamBytesFromPTY>(1);
        let (pty_input_tx, pty_input_rx) = mpsc::channel::<StreamBytesFromSTDIN>(1);
        let (protocol_tx, _) = tokio::sync::broadcast::channel(16);
        let protocol_rx_in = protocol_tx.subscribe();
        let protocol_rx_out = protocol_tx.subscribe();

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
            let pty = PTY::new(command).unwrap();
            pty.run(pty_input_rx, pty_output_tx, protocol_rx_in, protocol_rx_out)
                .await
                .unwrap();
            protocol_tx.send(Protocol::End).unwrap();
            tracing::debug!("Test PTY.run() done");
        });

        tracing::debug!("TEST: Leaving run helper...");
        (output_task, pty_input_tx)
    }

    fn stdin_bytes(input: &str) -> StreamBytesFromSTDIN {
        let mut buffer: StreamBytesFromSTDIN = [0; 128];
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
