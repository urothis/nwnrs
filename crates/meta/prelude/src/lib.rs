#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

/// Checksum types and helpers.
pub mod checksums {
    pub use nwnrs_checksums::prelude::*;
}

/// Compressed-buffer types and helpers.
pub mod compressedbuf {
    pub use nwnrs_compressedbuf::prelude::*;
}

/// Localization vocabulary and language helpers.
pub mod localization {
    pub use nwnrs_localization::prelude::*;
}

/// DDS texture types and helpers.
pub mod dds {
    pub use nwnrs_dds::prelude::*;
}

/// Text-encoding types and helpers.
pub mod encoding {
    pub use nwnrs_encoding::prelude::*;
}

/// ERF archive types and helpers.
pub mod erf {
    pub use nwnrs_erf::prelude::*;
}

/// EXO constants and compression markers.
pub mod exo {
    pub use nwnrs_exo::prelude::*;
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(docsrs, doc(cfg(not(target_arch = "wasm32"))))]
/// Install-discovery and install-bootstrap helpers.
pub mod install {
    pub use nwnrs_install::prelude::*;
}

/// GFF document types and helpers.
pub mod gff {
    pub use nwnrs_gff::prelude::*;
}

/// GIT area-instance types and helpers.
pub mod git {
    pub use nwnrs_git::prelude::*;
}

/// Binary IO, endian, and invariant helpers.
pub mod io {
    pub use nwnrs_io::prelude::*;
}

/// KEY/BIF archive types and helpers.
pub mod key {
    pub use nwnrs_key::prelude::*;
}

/// Weighted LRU cache types.
pub mod lru {
    pub use nwnrs_lru::prelude::*;
}

/// Beamdog masterlist API client types and helpers.
pub mod masterlist {
    pub use nwnrs_masterlist::prelude::*;
}

/// MDL model types and helpers.
pub mod mdl {
    pub use nwnrs_mdl::prelude::*;
}

/// MTR material types and helpers.
pub mod mtr {
    pub use nwnrs_mtr::prelude::*;
}

/// NWScript compiler and format types and helpers.
pub mod nwscript {
    pub use nwnrs_nwscript::prelude::*;
}

/// NWSync manifest types and helpers.
pub mod nwsync {
    pub use nwnrs_nwsync::prelude::*;
}

/// PLT texture types and helpers.
pub mod plt {
    pub use nwnrs_plt::prelude::*;
}

/// Resource-directory container types and helpers.
pub mod resdir {
    pub use nwnrs_resdir::prelude::*;
}

/// Single-file resource container types and helpers.
pub mod resfile {
    pub use nwnrs_resfile::prelude::*;
}

/// Resource-manager types and helpers.
pub mod resman {
    pub use nwnrs_resman::prelude::*;
}

/// In-memory resource container types and helpers.
pub mod resmemfile {
    pub use nwnrs_resmemfile::prelude::*;
}

/// NWSync-backed resource container types and helpers.
pub mod resnwsync {
    pub use nwnrs_resnwsync::prelude::*;
}

/// Resource-reference types and helpers.
pub mod resref {
    pub use nwnrs_resref::prelude::*;
}

/// Resource-type registry types and helpers.
pub mod restype {
    pub use nwnrs_restype::prelude::*;
}

/// Tileset `SET` types and helpers.
pub mod set {
    pub use nwnrs_set::prelude::*;
}

/// Soundset `SSF` types and helpers.
pub mod ssf {
    pub use nwnrs_ssf::prelude::*;
}

/// Stream extension traits and helpers.
pub mod streamext {
    pub use nwnrs_streamext::prelude::*;
}

/// TGA texture types and helpers.
pub mod tga {
    pub use nwnrs_tga::prelude::*;
}

/// TLK dialog-table types and helpers.
pub mod tlk {
    pub use nwnrs_tlk::prelude::*;
}

/// TXI texture-info types and helpers.
pub mod txi {
    pub use nwnrs_txi::prelude::*;
}

/// `2DA V2.0` table types and helpers.
pub mod twoda {
    pub use nwnrs_twoda::prelude::*;
}

/// Convenience namespace that re-exports the public crate modules.
///
/// Prefer the root modules such as [`crate::gff`] or [`crate::resman`] when you
/// want a stable, explicit import path. Use this namespace only when a single
/// wildcard import is materially more convenient.
pub mod prelude {
    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::install;
    pub use crate::{
        checksums, compressedbuf, dds, encoding, erf, exo, gff, git, io, key, localization, lru,
        masterlist, mdl, mtr, nwscript, nwsync, plt, resdir, resfile, resman, resmemfile,
        resnwsync, resref, restype, set, ssf, streamext, tga, tlk, twoda, txi,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_modules_expose_expected_entry_points() {
        let mut root = gff::GffRoot::new("UTC ");
        let put_result = root.put_value("Tag", gff::GffValue::CExoString("nw_chicken".to_string()));
        assert!(
            put_result.is_ok(),
            "gff root should accept tag field: {:?}",
            put_result.as_ref().err()
        );

        let mut table = twoda::TwoDa::new();
        let set_columns_result = table.set_columns(vec!["Label".to_string()]);
        assert!(
            set_columns_result.is_ok(),
            "2DA columns should be accepted: {:?}",
            set_columns_result.as_ref().err()
        );

        let _cache = io::ExpectationError::new("expected");

        assert_eq!(root.file_type, "UTC ");
        assert_eq!(table.columns(), &["Label".to_string()]);
    }

    #[test]
    fn prelude_namespace_reexports_root_modules() {
        let _gff_root = prelude::gff::GffRoot::new("ARE ");
        let _table = prelude::twoda::TwoDa::new();
        let _error = prelude::io::ExpectationError::new("left");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn prelude_exposes_install_on_native_targets() {
        let _finder: fn(&str) -> nwnrs_install::InstallResult<std::path::PathBuf> =
            prelude::install::find_nwnrs_root;
    }
}
