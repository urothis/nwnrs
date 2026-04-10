use std::{fmt, io};

use nwnrs_dds::prelude::DdsError;
use nwnrs_mdl::prelude::ModelError;
use nwnrs_mtr::prelude::MtrError;
use nwnrs_plt::prelude::PltError;
use nwnrs_tga::prelude::TgaError;

/// Errors returned while loading or converting NWN assets for Bevy.
#[derive(Debug)]
pub enum NwnBevyError {
    /// Reading source bytes failed.
    Io(io::Error),
    /// MDL parsing or lowering failed.
    Model(ModelError),
    /// TGA parsing or decode failed.
    Tga(TgaError),
    /// DDS parsing or decode failed.
    Dds(DdsError),
    /// MTR parsing failed.
    Mtr(MtrError),
    /// PLT parsing or render failed.
    Plt(PltError),
    /// The input asset was otherwise invalid.
    Message(String),
}

impl NwnBevyError {
    /// Creates a free-form Bevy integration error.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for NwnBevyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Model(error) => error.fmt(f),
            Self::Tga(error) => error.fmt(f),
            Self::Dds(error) => error.fmt(f),
            Self::Mtr(error) => error.fmt(f),
            Self::Plt(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for NwnBevyError {}

impl From<io::Error> for NwnBevyError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ModelError> for NwnBevyError {
    fn from(value: ModelError) -> Self {
        Self::Model(value)
    }
}

impl From<TgaError> for NwnBevyError {
    fn from(value: TgaError) -> Self {
        Self::Tga(value)
    }
}

impl From<DdsError> for NwnBevyError {
    fn from(value: DdsError) -> Self {
        Self::Dds(value)
    }
}

impl From<MtrError> for NwnBevyError {
    fn from(value: MtrError) -> Self {
        Self::Mtr(value)
    }
}

impl From<PltError> for NwnBevyError {
    fn from(value: PltError) -> Self {
        Self::Plt(value)
    }
}
