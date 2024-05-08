//! Create a PTY and send and recieve bytes over channels

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

use crate::run::Protocol;
use crate::shared_state::SharedState;

/// A single read/write from the PTY output stream
pub type StreamBytes = [u8; 128];

/// This is the TTY process that replaces the user's current TTY
#[non_exhaustive]
pub struct PTY {
    /// PTY height
    pub height: u16,
    /// PTY width
    pub width: u16,
    /// PTY starting command
    pub command: Vec<OsString>,
}

impl PTY {
    /// Docs
    pub fn new(state: &Arc<SharedState>, command: Vec<OsString>) -> Result<Self> {
        let tty_size = state.get_tty_size()?;

        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
        let pty = Self {
            width: tty_size.0 as u16,
            height: tty_size.1 as u16,
            command,
        };
        Ok(pty)
    }

    /// Function just to isolate the PTY setup
    fn setup_pty(&self) -> Result<(std::fs::File, portable_pty::PtyPair)> {
        tracing::debug!("Setting up PTY");
        let pty_system = portable_pty::native_pty_system();

        let pty_result = pty_system.openpty(portable_pty::PtySize {
            rows: self.height,
            cols: self.width,
            // Not all systems support pixel_width, pixel_height,
            // but it is good practice to set it to something
            // that matches the size of the selected font.
            pixel_width: 0,
            pixel_height: 0,
        });

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
        match pair.slave.spawn_command(cmd) {
            Ok(pty_ok) => pty_ok,
            Err(pty_error) => {
                // For some reason the error returned by `spawn_command` can't be handled
                // by the magic `?` operator. hence why I've manually unpacked it.
                let error = format!("Error spawning PTY command: {pty_error}");
                return Err(eyre!(error));
            }
        };

        // The only reason we need the raw file descriptor is because it allows the PTY
        // to close when it receives CTRL+D. I got the idea from this
        // [Github comment](https://github.com/wez/wezterm/discussions/5151)
        let Some(master_fd) = pair.master.as_raw_fd() else {
            return Err(eyre!("Couldn't get master file descriptor for PTY"));
        };

        tracing::debug!("Returning PTY file descriptor");
        Ok((
            // SAFETY: Why is this unsafe? Is there another safe way to do this?
            unsafe { std::fs::File::from_raw_fd(master_fd) },
            pair,
        ))
    }

    /// Start the PTY
    pub fn run(
        &self,
        user_input: mpsc::UnboundedReceiver<StreamBytes>,
        pty_output: mpsc::UnboundedSender<StreamBytes>,
        protocol: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        let (pty_stream, pty_pair) = self.setup_pty()?;
        let pty_stream_arc = Arc::new(pty_stream);
        let mut pty_stream_reader = Arc::clone(&pty_stream_arc);
        let pty_stream_writer = Arc::clone(&pty_stream_arc);

        // Release any handles owned by the slave: we don't need it now that we've spawned the child.
        // We need `pair.master` though as that keeps the PTY alive
        drop(pty_pair.slave);

        tokio::spawn(async move {
            #[allow(clippy::multiple_unsafe_ops_per_block)]
            if let Err(err) = Self::forward_input(user_input, pty_stream_writer, protocol).await {
                tracing::error!("Writing to PTY stream: {err}");
            }
        });

        tracing::debug!("Starting PTY reader loop");
        loop {
            let mut buffer: StreamBytes = [0; 128];

            #[allow(clippy::multiple_unsafe_ops_per_block)]
            let chunk_size = pty_stream_reader.read(&mut buffer[..]);
            match chunk_size {
                Ok(0) => {
                    tracing::debug!("PTY reader received 0 bytes");
                    break;
                }
                Ok(_) => {
                    let output = String::from_utf8_lossy(&buffer);
                    tracing::trace!("PTY output: \"{output:?}\"");

                    if let Err(err) = pty_output.send(buffer) {
                        tracing::error!("Sending bytes on PTY output channel: {err}");
                    };
                }
                Err(err) => {
                    // We don't want the internal `drop()` to be called on this because if we have
                    // an error in the PTY stream, we assume that it's already gone. I think it's
                    // to do with the unsafe `from_raw_fd` in the setup code.
                    #[allow(clippy::mem_forget)]
                    std::mem::forget(pty_stream_arc);

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

    /// Forward channel bytes from the user's STDIN to the virtual PTY
    async fn forward_input(
        mut user_input: mpsc::UnboundedReceiver<StreamBytes>,
        mut pty_stream: Arc<std::fs::File>,
        mut protocol: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        tracing::debug!("Starting `forward_input` loop");
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        loop {
            tokio::select! {
                message = protocol.recv() => {
                    match message {
                        // TODO: should this be oneshot?
                        Ok(Protocol::END) => {
                            tracing::trace!("STDIN forwarder task received {message:?}");
                            break;
                        }
                        Err(err) => {
                            return Err(color_eyre::eyre::Error::new(err));
                        }
                    };
                }
                some_bytes = user_input.recv() => {
                    if let Some(bytes) = some_bytes {
                        // Don't send unnecessary bytes, because `terminal.advance_bytes` parses them all
                        for byte in bytes {
                            if byte == 0 {
                                break;
                            }
                            pty_stream.write_all(&[byte])?;
                        }
                        pty_stream.flush()?;
                    }
                }
            }
        }

        tracing::debug!("STDIN forwarder loop finished");
        Ok(())
    }

    /// Redirect the main application's STDIN to the PTY process
    pub async fn consume_stdin(
        input: &mpsc::UnboundedSender<StreamBytes>,
        mut protocol: tokio::sync::broadcast::Receiver<Protocol>,
    ) -> Result<()> {
        tracing::debug!("Starting to listen on STDIN");

        let stdin = tokio::io::stdin();
        let mut reader = tokio::io::BufReader::new(stdin);

        #[allow(clippy::multiple_unsafe_ops_per_block)]
        loop {
            let mut buffer: StreamBytes = [0; 128];
            tokio::select! {
                message = protocol.recv() => {
                    // TODO: should this be oneshot?
                    match message {
                        Ok(Protocol::END) => {
                            tracing::trace!("STDIN task received {message:?}");
                            break;
                        }
                        Err(err) => {
                            return Err(color_eyre::eyre::Error::new(err));
                        }
                    };
                }
                byte_count = reader.read(&mut buffer[..]) => {
                    match byte_count {
                        Ok(n) => {
                            if n > 0 {
                                input.send(buffer)?;
                            }
                        }
                        Err(err) => {
                            return Err(color_eyre::eyre::Error::new(err));
                        }
                    }
                }
            }
        }

        tracing::debug!("STDIN consumer loop finished");
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::multiple_unsafe_ops_per_block)]
mod tests {
    use super::*;

    fn run(
        command: Vec<OsString>,
    ) -> (
        tokio::task::JoinHandle<std::string::String>,
        mpsc::UnboundedSender<StreamBytes>,
    ) {
        // TODO: Think about a convenient way to enable this whenever only a single test is ran
        // setup_logging().unwrap();

        let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<StreamBytes>();
        let (pty_input_tx, pty_input_rx) = mpsc::unbounded_channel::<StreamBytes>();
        let (protocol_tx, _) = tokio::sync::broadcast::channel(16);
        let protocol_rx = protocol_tx.subscribe();

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
            let state = Arc::new(SharedState::default());
            let pty = PTY::new(&state, command).unwrap();
            pty.run(pty_input_rx, pty_output_tx, protocol_rx).unwrap();
            protocol_tx.send(Protocol::END).unwrap();
            tracing::debug!("Test PTY.run() done");
        });

        tracing::debug!("TEST: Leaving run helper...");
        (output_task, pty_input_tx)
    }

    fn stdin_bytes(input: &str) -> StreamBytes {
        let mut buffer: StreamBytes = [0; 128];
        #[allow(clippy::indexing_slicing)]
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
            .unwrap();
        let result = output_task.await.unwrap();
        assert!(result.contains("Hello"));
    }
}
