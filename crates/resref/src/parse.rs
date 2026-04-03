use crate::{RESREF_MAX_LENGTH, ResRef, ResRefError, ResolvedResRef};
use nwn_restype::ResType;
use tracing::instrument;

/// Returns `true` if `value` is a valid NWN resource name.
pub fn is_valid_resref_part1(value: &str) -> bool {
    !value.is_empty() && value.len() <= RESREF_MAX_LENGTH
}

/// Creates a new resource reference.
#[instrument(level = "debug", skip_all, err, fields(res_type = res_type.0))]
pub fn new_res_ref(res_ref: impl Into<String>, res_type: ResType) -> Result<ResRef, ResRefError> {
    ResRef::new(res_ref, res_type)
}

/// Creates a new resolved resource reference.
#[instrument(level = "debug", skip_all, err, fields(res_type = res_type.0))]
pub fn new_resolved_res_ref(
    res_ref: impl Into<String>,
    res_type: ResType,
) -> Result<ResolvedResRef, ResRefError> {
    ResolvedResRef::new(res_ref, res_type)
}

/// Attempts to resolve a `name.ext` filename into a resource reference.
#[instrument(level = "debug", skip_all, fields(path = %filename))]
pub fn try_new_resolved_res_ref(filename: &str) -> Option<ResolvedResRef> {
    ResolvedResRef::try_from_filename(filename)
}

/// Resolves a `name.ext` filename into a resource reference.
#[instrument(level = "debug", skip_all, err, fields(path = %filename))]
pub fn new_resolved_res_ref_from_filename(filename: &str) -> Result<ResolvedResRef, ResRefError> {
    ResolvedResRef::from_filename(filename)
}
