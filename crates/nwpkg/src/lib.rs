#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod compiler_macros;
mod dependency;
mod error;
mod fs;
mod generation;
mod kind;
mod lock;
mod manifest;
mod manifest_document;
mod session;

pub use compiler_macros::load_dependency_macro_registry;
pub use dependency::{
    DEPENDENCY_LOCK_FORMAT, DependencyLock, DependencyOperation, DependencyOperationKind,
    DependencyProgressEvent, DependencyProgressReporter, DependencyResolverConfiguration,
    GitSelector, LockedDependency, LockedDependencySource, ResolvedIncludeDependency,
    clean_dependency_cache, clean_dependency_cache_with_cancellation, normalize_git_source,
    plan_dependency_operation, plan_dependency_operation_with_progress, read_dependency_lock,
    register_dependency_lock, resolve_dependency_operation,
    resolve_dependency_operation_with_progress, resolve_include_dependencies,
    resolve_include_dependencies_from_manifest,
    resolve_include_dependencies_from_manifest_with_cancellation,
    resolve_include_dependencies_from_manifest_with_configuration,
    resolve_include_dependencies_with_cancellation,
    resolve_include_dependencies_with_configuration, write_dependency_lock,
    write_registered_dependency_lock, write_registered_dependency_lock_with_cancellation,
};
pub use error::{NwpkgError, NwpkgErrorKind};
pub use fs::is_project_control_file;
pub use generation::{
    EventCompletionAnalysis, EventCompletionContext, EventCompletionKind, EventLaneCandidate,
    EventLanePlan, EventProjectPlan, EventRegistration, EventRelation, EventSourceRange,
    GeneratedEventDispatcher, GeneratedNwScriptSource,
    analyze_event_completion_from_session_with_cancellation,
    analyze_event_completion_with_overlays_and_cancellation,
    analyze_event_project_with_overlays_and_cancellation, generate_event_dispatcher,
    generate_event_dispatcher_from_session,
    generate_event_dispatcher_from_session_with_cancellation,
    generate_event_dispatcher_with_cancellation, generate_event_dispatcher_with_diagnostics,
    generate_event_dispatcher_with_overlays,
    generate_event_dispatcher_with_overlays_and_cancellation,
};
pub use kind::{ProjectKind, ProjectLayout};
pub use lock::{
    ErfPackMetadata, KeyPackMetadata, ResourcePackMetadata, copy_original_key_set,
    copy_original_key_set_with_cancellation, read_erf_pack_metadata,
    read_erf_pack_metadata_with_cancellation, read_key_pack_metadata,
    read_key_pack_metadata_with_cancellation, read_resource_pack_metadata,
    read_resource_pack_metadata_with_cancellation, resolve_existing_key_bif_path,
    should_copy_original_erf, should_copy_original_erf_with_cancellation, should_copy_original_key,
    should_copy_original_key_with_cancellation, should_copy_original_resource,
    should_copy_original_resource_with_cancellation, write_erf_pack_metadata,
    write_erf_pack_metadata_with_cancellation, write_key_pack_metadata,
    write_key_pack_metadata_with_cancellation, write_new_erf_pack_metadata,
    write_new_erf_pack_metadata_with_cancellation, write_new_key_pack_metadata,
    write_new_key_pack_metadata_with_cancellation, write_new_resource_pack_metadata,
    write_new_resource_pack_metadata_with_cancellation, write_resource_pack_metadata,
    write_resource_pack_metadata_with_cancellation,
};
pub use manifest::{
    DependencySpec, GitDependency, ManifestFieldSchema, ManifestSectionSchema, ManifestValueKind,
    PathDependency, ProjectLanguage, ProjectManifest, manifest_schema, read_project_manifest,
    read_project_manifest_with_cancellation, write_project_manifest,
    write_project_manifest_with_language,
};
pub use manifest_document::{
    ManifestDocument, ManifestEntry, ManifestSpan, ManifestSyntaxDiagnostic, ManifestTable,
};
pub use session::{ResolvedPackageSession, find_project_root, find_project_root_with_cancellation};

/// Canonical `nwpkg.toml` filename.
pub const PROJECT_MANIFEST_FILENAME: &str = "nwpkg.toml";

/// Canonical `nwpkg.lock` filename.
pub const PACKAGE_LOCK_FILENAME: &str = "nwpkg.lock";
