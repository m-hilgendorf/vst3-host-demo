use std::fmt;
use vst3::Steinberg::{
    kInternalError, kInvalidArgument, kNoInterface, kNotImplemented, kNotInitialized, kOutOfMemory,
    kResultFalse, kResultOk, tresult,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Error {
    False = kResultFalse,
    Internal = kInternalError,
    InvalidArg = kInvalidArgument,
    NoInterface = kNoInterface,
    NotImplemented = kNotImplemented,
    OutOfMemory = kOutOfMemory,
    NotInitialized = kNotInitialized,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::False => write!(f, "False."),
            Self::Internal => write!(f, "Internal error."),
            Self::InvalidArg => write!(f, "Invalid argument."),
            Self::NoInterface => write!(f, "No interface."),
            Self::NotImplemented => write!(f, "Not implemented."),
            Self::OutOfMemory => write!(f, "Out of memory."),
            Self::NotInitialized => write!(f, "Not initialized."),
        }
    }
}

impl From<Error> for tresult {
    fn from(value: Error) -> Self {
        value as tresult
    }
}

pub(crate) trait ToCodeExt {
    fn to_code(&self) -> tresult;
}

impl ToCodeExt for Result<(), Error> {
    fn to_code(&self) -> tresult {
        match self {
            Ok(()) => kResultOk,
            Err(e) => (*e).into(),
        }
    }
}

pub(crate) trait ToResultExt {
    fn as_result(&self) -> Result<(), Error>;
}

impl ToResultExt for tresult {
    #[allow(non_upper_case_globals)]
    fn as_result(&self) -> Result<(), Error> {
        match *self {
            0 => Ok(()),
            kInternalError => Err(Error::Internal),
            kInvalidArgument => Err(Error::InvalidArg),
            kNoInterface => Err(Error::NoInterface),
            kNotImplemented => Err(Error::NotImplemented),
            kOutOfMemory => Err(Error::OutOfMemory),
            kNotInitialized => Err(Error::NotInitialized),
            _ => Err(Error::False),
        }
    }
}
