//! Handle all the raw input directly from the end user.

use std::io::Read as _;

use color_eyre::eyre::Result;

/// Bytes from STDIN
pub type BytesFromSTDIN = [u8; 128];

/// Input from STDIN that has been parsed into known mouse/keyboard/etc events.
#[derive(Debug, Clone)]
pub(crate) struct ParsedInput {
    /// The raw bytes that made up the parsed event
    pub bytes: BytesFromSTDIN,
    /// The parsed event
    pub event: termwiz::input::InputEvent,
}

/// Handle input from the user
pub(crate) struct Input {
    /// The main Tattoy protocol channel.
    protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
}

impl Input {
    /// Start a thread to listen and parse the end user's STDIN and forward it to the rest of the
    /// application.
    pub fn start(
        protocol_tx: tokio::sync::broadcast::Sender<crate::run::Protocol>,
    ) -> std::thread::JoinHandle<std::result::Result<(), color_eyre::eyre::Error>> {
        // The Tokio docs actually suggest using `std::thread` to listen on STDIN for interactive
        // applications.
        std::thread::spawn(move || -> Result<()> {
            let protocol_for_shutdown = protocol_tx.clone();
            let input = Self { protocol_tx };
            let result = input.consume_stdin();
            if let Err(error) = result {
                crate::run::broadcast_protocol_end(&protocol_for_shutdown);
                return Err(error);
            }
            Ok(())
        })
    }

    /// Listen to the end user's STDIN. Try to parse all the bytes, and if any Tattoy-specific
    /// mouse or keyboard events are detected, handle them seperately.
    fn consume_stdin(&self) -> Result<()> {
        tracing::debug!("Starting to listen on STDIN");

        let stdin = std::io::stdin();
        let mut reader = std::io::BufReader::new(stdin);
        let mut parser = termwiz::input::InputParser::new();

        loop {
            let mut buffer: BytesFromSTDIN = [0; 128];
            match reader.read(&mut buffer[..]) {
                Ok(n) => {
                    if let Some(bytes) = buffer.get(0..n) {
                        let sample = String::from_utf8_lossy(&buffer);
                        tracing::trace!("Received STDIN input: {sample} ({bytes:x?})");

                        parser.parse(
                            bytes,
                            |event| {
                                self.parsed_bytes_callback(event, buffer);
                            },
                            false,
                        );
                    } else {
                        tracing::warn!("Couldn't get bytes from STDIN input buffer");
                    }
                }
                Err(err) => {
                    return Err(color_eyre::eyre::Error::new(err));
                }
            }
        }
    }

    /// The callback for when the input parser detects known keyboard/mouse events.
    fn parsed_bytes_callback(&self, event: termwiz::input::InputEvent, bytes: BytesFromSTDIN) {
        tracing::trace!("Parsed input event: {event:?} ({bytes:x?})",);

        let result = self
            .protocol_tx
            .send(crate::run::Protocol::Input(ParsedInput { bytes, event }));
        if let Err(error) = result {
            tracing::error!("Error sending input event from thread to task: {error:?}");
        }
    }
}
