use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    sync::Arc,
    thread,
};

use nwnrs_types::resman::prelude::{ResType, get_res_ext};
use serde::{Deserialize, Serialize};

use crate::{
    CompileArtifacts, CompilerSession, CompilerSessionError, CompilerSessionOptions,
    NW_SCRIPT_BINARY_RES_TYPE, NW_SCRIPT_DEBUG_RES_TYPE, NW_SCRIPT_SOURCE_RES_TYPE, ScriptResolver,
    SourceError, canonicalize_with_missing_suffix, session::PreparedScript,
};

mod graphviz_output;
mod output;

#[cfg(test)]
use graphviz_output::publish_artifacts_transactionally;
use output::{PendingOutput, validate_logical_output_alias, validate_output_target};

mod batch;
mod diagnostics;
mod directory_host;
mod host;
mod resolver;

pub use batch::*;
#[cfg(test)]
use diagnostics::visible_diagnostic_span;
pub use diagnostics::*;
pub use directory_host::*;
pub use host::*;
pub use resolver::*;
#[cfg(test)]
mod tests;
