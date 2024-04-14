/// Create a PTY and send and recieve bytes over channels
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::sync::{self, Arc};

use color_eyre::eyre::{eyre, Result};
use tokio::sync::mpsc;

/// A single read/write from the PTY output stream
pub type StreamBytes = [u8; 128];

/// This is the TTY process that replaces the user's current TTY
#[non_exhaustive]
pub struct PTY {
    /// PTY height
    pub height: u16,
    /// PTY width
    pub width: u16,
    /// PTY statrting command
    pub command: Vec<OsString>,
}

impl PTY {
    /// Docs
    pub fn new(height: u16, width: u16, command: Vec<OsString>) -> Result<Self> {
        let pty = Self {
            height,
            width,
            command,
        };
        Ok(pty)
    }

    /// Just isolate the PTY setup
    fn setup_pty(&self) -> Result<(sync::Arc<fs::File>, portable_pty::PtyPair)> {
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
        let cmd = portable_pty::CommandBuilder::from_argv(self.command.clone());
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
            Arc::new(
                // SAFETY: Why is this unsafe? Is there another safe way to do this?
                unsafe { File::from_raw_fd(master_fd) },
            ),
            pair,
        ))
    }

    /// Start the PTY
    pub fn run(
        &self,
        input: mpsc::UnboundedReceiver<StreamBytes>,
        output: mpsc::UnboundedSender<StreamBytes>,
    ) -> Result<()> {
        let (mut pty_stream, pty_pair) = self.setup_pty()?;
        let pty_stream_input = Arc::clone(&pty_stream);

        // Release any handles owned by the slave: we don't need it now
        // that we've spawned the child.
        // We need `pair.master` though as that keeps the PTY alive
        drop(pty_pair.slave);

        std::thread::spawn(|| {
            if let Err(err) = Self::forward_input(input, pty_stream_input) {
                tracing::error!("Writing to PTY stream: {err}");
            }
        });

        loop {
            let mut buffer: StreamBytes = [0; 128];
            let chunk = pty_stream.read(&mut buffer[..]);
            match chunk {
                Ok(n) => {
                    if n > 0 {
                        if let Err(err) = output.send(buffer) {
                            tracing::error!("Sending bytes on PTY output channel: {err}");
                            continue;
                        };
                    }
                }
                Err(err) => {
                    tracing::error!("Reading PTY stream: {err}");
                    break;
                }
            }
        }

        // Required to close whatever loop is listening to the output
        drop(output);
        Ok(())
    }

    /// Forward channel bytes from the user's STDIN to the virtual PTY
    fn forward_input(
        mut input: mpsc::UnboundedReceiver<StreamBytes>,
        mut pty_stream: sync::Arc<fs::File>,
    ) -> Result<()> {
        loop {
            if let Some(bytes) = input.blocking_recv() {
                // Don't send unnecessary bytes as `terminal.advance_bytes` parses them all
                for byte in bytes {
                    if byte == 0 {
                        break;
                    }
                    pty_stream.write_all(&[byte])?;
                }
                pty_stream.flush()?;
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        command: Vec<OsString>,
    ) -> (
        std::thread::JoinHandle<std::string::String>,
        mpsc::UnboundedSender<StreamBytes>,
    ) {
        let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<StreamBytes>();
        let (pty_input_tx, pty_input_rx) = mpsc::unbounded_channel::<StreamBytes>();

        let output_thread = std::thread::spawn(move || {
            let mut result: Vec<u8> = vec![];
            while let Some(bytes) = pty_output_rx.blocking_recv() {
                result.extend(bytes.iter().copied());
            }
            String::from_utf8_lossy(&result).into_owned()
        });

        let pty = PTY::new(100, 100, command).unwrap();
        std::thread::spawn(move || {
            pty.run(pty_input_rx, pty_output_tx).unwrap();
        });

        (output_thread, pty_input_tx)
    }

    fn stdin_bytes(input: &str) -> StreamBytes {
        let mut buffer: StreamBytes = [0; 128];
        #[allow(clippy::indexing_slicing)]
        buffer[..input.len()].copy_from_slice(input.as_bytes());
        buffer
    }

    #[test]
    fn rendering_pi() {
        let (output_thread, _input_channel) = run(vec![
            "bash".into(),
            "-c".into(),
            "echo 'scale=10; 4*a(1)' | bc -l".into(),
        ]);
        let result = output_thread.join().unwrap();
        assert!(result.contains("3.1415926532"));
    }

    #[test]
    fn interactive() {
        let (output_thread, input_channel) = run(vec!["bash".into()]);
        input_channel
            .send(stdin_bytes("echo Hello && exit\n"))
            .unwrap();
        let result = output_thread.join().unwrap();
        assert!(result.contains("Hello"));
    }
}
