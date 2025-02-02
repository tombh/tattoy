//! Handle all the raw input directly from the end user.

use std::io::Read as _;

use color_eyre::eyre::Result;
use tokio::sync::mpsc;

use crate::pty::StreamBytesFromSTDIN;

/// Handle input from the user
pub(crate) struct Input;

impl Input {
    // TODO: If we want any kind of Tattoy-specific keybindings, I think this is the place to put
    // them.
    //
    /// Forward the tattoy application's STDIN to the PTY process
    pub fn consume_stdin(user_input: &mpsc::Sender<StreamBytesFromSTDIN>) -> Result<()> {
        tracing::debug!("Starting to listen on STDIN");

        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin);

        loop {
            let mut buffer: StreamBytesFromSTDIN = [0; 128];
            match reader.read(&mut buffer[..]) {
                Ok(n) => {
                    if n > 0 {
                        user_input.try_send(buffer)?;
                    }
                }
                Err(err) => {
                    return Err(color_eyre::eyre::Error::new(err));
                }
            }
        }
    }
}
