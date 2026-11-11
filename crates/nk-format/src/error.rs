use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Truncated { needed: usize, len: usize },
    UnsupportedMagic { magic: u32 },
    NoPayload,
    PayloadTooLarge { limit: usize },
    NotUtf8(std::str::Utf8Error),
    Xml(quick_xml::Error),
    Malformed(String),
    UnexpectedRoot { found: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Truncated { needed, len } => {
                write!(f, "truncated input: need {needed} bytes, have {len}")
            }
            Error::UnsupportedMagic { magic } => {
                write!(f, "unsupported container magic 0x{magic:08X}")
            }
            Error::NoPayload => write!(
                f,
                "no zlib-compressed K2 XML payload found (encrypted or unknown variant)"
            ),
            Error::PayloadTooLarge { limit } => {
                write!(f, "decompressed payload exceeds {limit} bytes")
            }
            Error::NotUtf8(e) => write!(f, "payload is not UTF-8: {e}"),
            Error::Xml(e) => write!(f, "XML error: {e}"),
            Error::Malformed(m) => write!(f, "malformed document: {m}"),
            Error::UnexpectedRoot { found } => {
                write!(f, "unexpected XML root <{found}>, expected <K2_Container>")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Xml(e) => Some(e),
            Error::NotUtf8(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<quick_xml::Error> for Error {
    fn from(e: quick_xml::Error) -> Self {
        Error::Xml(e)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Self {
        Error::NotUtf8(e)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Error::NotUtf8(e.utf8_error())
    }
}
