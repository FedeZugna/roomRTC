use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum SdpError {
    BadLine(String),
    BadVersion(String),
    MissingOrigin,
    MissingSessionName,
    MissingTiming,
    BadMedia(String),
    BadConnection(String),
    MissingConnectionForMedia(String),
    ParseInt(std::num::ParseIntError),
}

impl fmt::Display for SdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SdpError::BadLine(s) => write!(f, "línea SDP inválida: {s}"),
            SdpError::BadVersion(s) => write!(f, "versión SDP inválida: {s}"),
            SdpError::MissingOrigin => write!(f, "falta origen (o=)"),
            SdpError::MissingSessionName => write!(f, "falta session name (s=)"),
            SdpError::MissingTiming => write!(f, "falta timing (t=)"),
            SdpError::BadMedia(s) => write!(f, "media inválida: {s}"),
            SdpError::BadConnection(s) => write!(f, "connection inválida: {s}"),
            SdpError::MissingConnectionForMedia(s) => {
                write!(f, "sin conexión (c=) para media: {s}")
            }
            SdpError::ParseInt(e) => write!(f, "error numérico: {e}"),
        }
    }
}

impl Error for SdpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let SdpError::ParseInt(e) = self {
            Some(e)
        } else {
            None
        }
    }
}

impl From<std::num::ParseIntError> for SdpError {
    fn from(e: std::num::ParseIntError) -> Self {
        SdpError::ParseInt(e)
    }
}
