//! Handle all the raw input directly from the end user.

use std::io::Read as _;

use color_eyre::eyre::Result;
use tokio::sync::mpsc;

/// Bytes from STDIN
pub type BytesFromSTDIN = [u8; 128];

/// Handle input from the user
pub(crate) struct Input;

impl Input {
    // TODO: If we want any kind of Tattoy-specific keybindings, I think this is the place to put
    // them.
    //
    /// Forward the tattoy application's STDIN to the PTY process
    pub fn consume_stdin(user_input: &mpsc::Sender<BytesFromSTDIN>) -> Result<()> {
        tracing::debug!("Starting to listen on STDIN");

        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin);

        loop {
            let mut buffer: BytesFromSTDIN = [0; 128];
            match reader.read(&mut buffer[..]) {
                Ok(n) => {
                    if n > 0 {
                        let sample = String::from_utf8_lossy(&buffer);
                        tracing::trace!("Forwarding STDIN input: {sample}");

                        // TODO:
                        // 1. Loop through 1 byte at a time, waiting for a valid parsed event.
                        // 2. Keep track of all the bytes that have contributed to a possible
                        //    valid event.
                        // 3. If it's a known Tattoy-specific `termwiz::input::InputEvent`
                        //    then act on it and don't forward those bytes.
                        // 4. If no Tattoy-specific event is parsed then forward the bytes.
                        //
                        // * Will need a new channel to send Tattoy-specific events over.
                        // * Will need to add `is_alternate_screen_active` to shared state.
                        // * Possible Tattoy-specific events:
                        //   * Scrolling when alternate screen is not active.
                        //   * Toggling tattoys on and off
                        //   * Keybindings specific to individual tattoys
                        //
                        // TODO:
                        // This approach depends on the guarantee that a read from
                        // `reader.read` will never truncate the bytes required to parse
                        // an event.
                        //
                        // One approach to solve this would be to somehow learn the exact
                        // bytes that go up to form a give `InputEvent`. With those bytes
                        // you could delete from the stream.
                        //
                        // let parser = termwiz::input::InputParser::new();
                        // parser.parse();
                        user_input.try_send(buffer)?;
                    }
                }
                Err(err) => {
                    return Err(color_eyre::eyre::Error::new(err));
                }
            }
        }
    }

    /// Run the thread that listens to the user's STDIN.
    pub fn start(
        pty_input_tx: mpsc::Sender<BytesFromSTDIN>,
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> std::thread::JoinHandle<std::result::Result<(), color_eyre::eyre::Error>> {
        std::thread::spawn(move || -> Result<()> {
            let result = Self::consume_stdin(&pty_input_tx);
            if let Err(error) = result {
                crate::run::broadcast_protocol_end(&protocol_tx);
                return Err(error);
            };
            Ok(())
        })
    }
}
