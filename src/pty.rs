use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::process::exit;
use std::sync::{self, Arc};

use color_eyre::eyre::{eyre, Result};
use portable_pty::PtyPair;
use tokio::sync::mpsc;

/// A single read from the PTY output stream
type StreamBytes = [u8; 128];

/// This is the TTY process that replaces the user's current TTY
pub struct PTY {
    /// Height/rows of the TTY
    height: u16,
    /// Width/columns of the TTY
    width: u16,
    /// The command to run: `bash`, `zsh`, etc
    shell: String,
    /// A channel to send messages between threads
    output: mpsc::UnboundedSender<StreamBytes>,
}

impl PTY {
    /// Docs
    #[must_use]
    pub const fn new(
        height: u16,
        width: u16,
        shell: String,
        output: mpsc::UnboundedSender<StreamBytes>,
    ) -> Self {
        Self {
            height,
            width,
            shell,
            output,
        }
    }

    /// Start the PTY
    /// # Panics
    pub fn run(&self) -> Result<()> {
        let (mut pty_stream, pair) = self.setup_pty()?;

        // Release any handles owned by the slave: we don't need it now
        // that we've spawned the child.
        // We need `pair.master` though as that keeps the PTY alive
        drop(pair.slave);

        let pty_stream_clone = Arc::clone(&pty_stream);
        let output = self.output.clone();

        tokio::spawn(async move {
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

            // TODO: this exits the entire app. Is that what we want?
            exit(0);
        });

        Self::process_stdin(pty_stream_clone)?;
        Ok(())
    }

    /// Doc
    fn setup_pty(&self) -> Result<(sync::Arc<fs::File>, PtyPair)> {
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

        let cmd = portable_pty::CommandBuilder::new(self.shell.clone());

        tracing::debug!("Launching `{}` on PTY", self.shell);
        _ = match pair.slave.spawn_command(cmd) {
            Ok(pty_ok) => pty_ok,
            Err(pty_error) => {
                // For some reason the error returned by `spawn_command` can't be handled
                // by the magic `?` operator. hence why I've manually unpacked it.
                let error = format!("Error spawning PTY command: {pty_error}");
                return Err(eyre!(error));
            }
        };

        Self::enter_raw_mode(0)?;

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

    /// Do all the TTY magic to get a clean terminal with the features we need
    /// See: [Github comment](https://github.com/wez/wezterm/discussions/5151)
    ///
    /// TODO:
    ///   * Don't we already have a function from one of our dependencies that can do this?
    ///   * Automatically undo this on application exit
    fn enter_raw_mode(fd: i32) -> Result<()> {
        tracing::debug!("Putting current user TTY into RAW mode");

        let mut new_termios = (termios::Termios::from_fd(fd))?;

        new_termios.c_lflag &= !(
            // Allows the PTY to get CTRL+C/D signals
            termios::ISIG |
        // TODO: document th others
        termios::ECHO | termios::ICANON | termios::IEXTEN
        );
        new_termios.c_iflag &=
            !(termios::BRKINT | termios::ICRNL | termios::INPCK | termios::ISTRIP | termios::IXON);
        new_termios.c_cflag &= !(termios::CSIZE | termios::PARENB);
        new_termios.c_cflag |= termios::CS8;
        new_termios.c_oflag &= !(termios::OPOST);
        new_termios.c_cc[termios::VMIN] = 1;
        new_termios.c_cc[termios::VTIME] = 0;

        termios::tcsetattr(0, termios::TCSANOW, &new_termios)?;

        Ok(())
    }

    /// This is the direct STDIN of the end user
    fn process_stdin(mut pty_stream: Arc<File>) -> Result<()> {
        tracing::debug!("Starting to listen on STDIN");

        let mut buffer = [0; 128];

        loop {
            let n = io::stdin().lock().read(&mut buffer[..])?;
            if n > 0 {
                if let Some(chunk) = buffer.get(..n) {
                    pty_stream.write_all(chunk)?;
                    pty_stream.flush()?;
                }
            }
        }
    }
}
