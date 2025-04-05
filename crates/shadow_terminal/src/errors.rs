//! Errors for this library
//
// TODO: This is my first use of Snafu, there seems like a lot of `Whatever` boilerplate?

/// All the known errors returned by this crate.
#[derive(Debug, snafu::Snafu)]
#[non_exhaustive]
pub enum ShadowTerminalError {
    #[snafu(display("PTY Error"))]
    /// Any error that occurs in the PTY
    PTY {
        /// The parent error type
        source: PTYError,
    },

    #[snafu(display("SteppableTerminal Error"))]
    /// Any error that occurs in the PTY
    SteppableTerminal {
        /// The parent error type
        source: SteppableTerminalError,
    },

    /// General errors that don't need to be matched on
    #[snafu(whatever, display("{message}"))]
    Whatever {
        /// A helpful message acompanying the error
        message: String,
        /// The parent error type
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

/// An error in the PTY.
#[derive(Debug, snafu::Snafu)]
#[non_exhaustive]
pub enum PTYError {
    /// General errors that don't need to be matched on
    #[snafu(whatever, display("{message}"))]
    Whatever {
        /// A helpful message acompanying the error
        message: String,
        /// The parent error type
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

/// An error in the Steppable terminal.
#[derive(Debug, snafu::Snafu)]
#[non_exhaustive]
pub enum SteppableTerminalError {
    /// General errors that don't need to be matched on
    #[snafu(whatever, display("{message}"))]
    Whatever {
        /// A helpful message acompanying the error
        message: String,
        /// The parent error type
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}
