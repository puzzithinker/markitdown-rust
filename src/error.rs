use std::fmt;

/// One converter that accepted the input and then failed.
#[derive(Debug, Clone)]
pub struct FailedAttempt {
    /// Converter name, such as `pdf` or `csv`.
    pub converter: &'static str,
    /// The error that converter returned.
    pub message: String,
}

/// Why a conversion failed.
#[derive(Debug)]
pub enum Error {
    /// Reading or writing bytes failed.
    Io(std::io::Error),
    /// No converter accepted the input.
    UnsupportedFormat(String),
    /// At least one converter accepted the input, and every attempt failed.
    FileConversion(Vec<FailedAttempt>),
    /// A URI could not be fetched, or the target was rejected.
    Fetch(String),
    /// A converter found a problem in a file it otherwise recognized.
    Message(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "{err}"),
            Error::UnsupportedFormat(message) | Error::Fetch(message) | Error::Message(message) => {
                write!(f, "{message}")
            }
            Error::FileConversion(attempts) => {
                writeln!(
                    f,
                    "File conversion failed after {} attempts:",
                    attempts.len()
                )?;
                for attempt in attempts {
                    writeln!(f, "- {}: {}", attempt.converter, attempt.message)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::{Error, FailedAttempt};

    #[test]
    fn file_conversion_lists_attempts_and_io_keeps_its_source() {
        let err = Error::FileConversion(vec![FailedAttempt {
            converter: "csv",
            message: "bad row".into(),
        }]);
        let text = err.to_string();
        assert!(text.contains("1 attempts"));
        assert!(text.contains("- csv: bad row"));
        assert!(text.ends_with('\n'));
        assert!(std::error::Error::source(&err).is_none());

        let io = Error::from(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        assert_eq!(io.to_string(), "missing");
        assert!(std::error::Error::source(&io).is_some());
    }
}
