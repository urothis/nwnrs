#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

extern crate self as nwnrs_types;

#[cfg(feature = "checksums")]
#[cfg_attr(docsrs, doc(cfg(feature = "checksums")))]
/// Checksum types and helpers.
pub mod checksums;

#[cfg(feature = "compressedbuf")]
#[cfg_attr(docsrs, doc(cfg(feature = "compressedbuf")))]
/// Compressed-buffer types and helpers.
pub mod compressedbuf;

#[cfg(feature = "dds")]
#[cfg_attr(docsrs, doc(cfg(feature = "dds")))]
/// DDS texture types and helpers.
pub mod dds;

#[cfg(feature = "encoding")]
#[cfg_attr(docsrs, doc(cfg(feature = "encoding")))]
/// Text-encoding types and helpers.
pub mod encoding;

#[cfg(feature = "erf")]
#[cfg_attr(docsrs, doc(cfg(feature = "erf")))]
/// ERF archive types and helpers.
pub mod erf;

#[cfg(feature = "exo")]
#[cfg_attr(docsrs, doc(cfg(feature = "exo")))]
/// EXO constants and compression markers.
pub mod exo;

#[cfg(feature = "gff")]
#[cfg_attr(docsrs, doc(cfg(feature = "gff")))]
/// GFF document types and helpers.
pub mod gff;

#[cfg(all(feature = "install", not(target_arch = "wasm32")))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(feature = "install", not(target_arch = "wasm32"))))
)]
/// Install-discovery and install-bootstrap helpers.
pub mod install;

#[cfg(feature = "io")]
#[cfg_attr(docsrs, doc(cfg(feature = "io")))]
/// Binary IO, endian, and invariant helpers.
pub mod io;

#[cfg(feature = "key")]
#[cfg_attr(docsrs, doc(cfg(feature = "key")))]
/// KEY/BIF archive types and helpers.
pub mod key;

#[cfg(feature = "localization")]
#[cfg_attr(docsrs, doc(cfg(feature = "localization")))]
/// Localization vocabulary and language helpers.
pub mod localization;

#[cfg(feature = "lru")]
#[cfg_attr(docsrs, doc(cfg(feature = "lru")))]
/// Weighted LRU cache types.
pub mod lru;

#[cfg(feature = "masterlist")]
#[cfg_attr(docsrs, doc(cfg(feature = "masterlist")))]
/// Beamdog masterlist API client types and helpers.
pub mod masterlist;

#[cfg(feature = "mdl")]
#[cfg_attr(docsrs, doc(cfg(feature = "mdl")))]
/// MDL model types and helpers.
pub mod mdl;

#[cfg(feature = "mtr")]
#[cfg_attr(docsrs, doc(cfg(feature = "mtr")))]
/// MTR material types and helpers.
pub mod mtr;

#[cfg(feature = "nwsync")]
#[cfg_attr(docsrs, doc(cfg(feature = "nwsync")))]
/// `NWSync` manifest types and helpers.
pub mod nwsync;

#[cfg(feature = "plt")]
#[cfg_attr(docsrs, doc(cfg(feature = "plt")))]
/// PLT texture types and helpers.
pub mod plt;

#[cfg(feature = "resman")]
#[cfg_attr(docsrs, doc(cfg(feature = "resman")))]
/// Resource-manager types and helpers.
pub mod resman;

#[cfg(feature = "set")]
#[cfg_attr(docsrs, doc(cfg(feature = "set")))]
/// Tileset `SET` types and helpers.
pub mod set;

#[cfg(feature = "ssf")]
#[cfg_attr(docsrs, doc(cfg(feature = "ssf")))]
/// Soundset `SSF` types and helpers.
pub mod ssf;

#[cfg(feature = "streamext")]
#[cfg_attr(docsrs, doc(cfg(feature = "streamext")))]
/// Stream extension traits and helpers.
pub mod streamext;

#[cfg(feature = "tga")]
#[cfg_attr(docsrs, doc(cfg(feature = "tga")))]
/// TGA texture types and helpers.
pub mod tga;

#[cfg(all(feature = "install", not(target_arch = "wasm32")))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(feature = "install", not(target_arch = "wasm32"))))
)]
#[doc(hidden)]
pub mod test_support {
    pub use crate::install::test_support::*;
}

#[cfg(feature = "tlk")]
#[cfg_attr(docsrs, doc(cfg(feature = "tlk")))]
/// TLK dialog-table types and helpers.
pub mod tlk;

#[cfg(feature = "twoda")]
#[cfg_attr(docsrs, doc(cfg(feature = "twoda")))]
/// `2DA V2.0` table types and helpers.
pub mod twoda;

#[cfg(feature = "txi")]
#[cfg_attr(docsrs, doc(cfg(feature = "txi")))]
/// TXI texture-info types and helpers.
pub mod txi;

/// Convenience namespace that re-exports the public crate modules.
///
/// Prefer the root modules such as [`crate::gff`] or [`crate::resman`] when you
/// want a stable, explicit import path. Use this namespace only when a single
/// wildcard import is materially more convenient.
pub mod prelude {
    #[cfg(feature = "checksums")]
    pub use crate::checksums;
    #[cfg(feature = "compressedbuf")]
    pub use crate::compressedbuf;
    #[cfg(feature = "dds")]
    pub use crate::dds;
    #[cfg(feature = "encoding")]
    pub use crate::encoding;
    #[cfg(feature = "erf")]
    pub use crate::erf;
    #[cfg(feature = "exo")]
    pub use crate::exo;
    #[cfg(feature = "gff")]
    pub use crate::gff;
    #[cfg(all(feature = "install", not(target_arch = "wasm32")))]
    pub use crate::install;
    #[cfg(feature = "io")]
    pub use crate::io;
    #[cfg(feature = "key")]
    pub use crate::key;
    #[cfg(feature = "localization")]
    pub use crate::localization;
    #[cfg(feature = "lru")]
    pub use crate::lru;
    #[cfg(feature = "masterlist")]
    pub use crate::masterlist;
    #[cfg(feature = "mdl")]
    pub use crate::mdl;
    #[cfg(feature = "mtr")]
    pub use crate::mtr;
    #[cfg(feature = "nwsync")]
    pub use crate::nwsync;
    #[cfg(feature = "plt")]
    pub use crate::plt;
    #[cfg(feature = "resman")]
    pub use crate::resman;
    #[cfg(feature = "set")]
    pub use crate::set;
    #[cfg(feature = "ssf")]
    pub use crate::ssf;
    #[cfg(feature = "streamext")]
    pub use crate::streamext;
    #[cfg(feature = "tga")]
    pub use crate::tga;
    #[cfg(feature = "tlk")]
    pub use crate::tlk;
    #[cfg(feature = "twoda")]
    pub use crate::twoda;
    #[cfg(feature = "txi")]
    pub use crate::txi;
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

    #[test]
    fn serializable_types_expose_read_and_write_entry_points() {
        type Cursor = std::io::Cursor<Vec<u8>>;

        let _compressedbuf_read: fn(
            &[u8],
            u32,
        ) -> compressedbuf::CompressedBufResult<
            compressedbuf::CompressedBufPayload,
        > = compressedbuf::read_payload_bytes;
        let _compressedbuf_write: fn(
            &compressedbuf::CompressedBufPayload,
        ) -> compressedbuf::CompressedBufResult<Vec<u8>> = compressedbuf::write_payload_bytes;

        let _dds_read: fn(&mut Cursor) -> dds::DdsResult<dds::DdsTexture> = dds::read_dds::<Cursor>;
        let _dds_write: fn(&mut Cursor, &dds::DdsTexture) -> dds::DdsResult<()> =
            dds::write_dds::<Cursor>;

        let _erf_read: fn(std::path::PathBuf) -> erf::ErfResult<erf::Erf> = erf::read_erf_from_file;
        let _erf_write: fn(&mut Cursor, &erf::Erf) -> erf::ErfResult<()> =
            erf::write_erf_archive::<Cursor>;

        let _gff_read: fn(&mut Cursor) -> gff::GffResult<gff::GffRoot> =
            gff::read_gff_root::<Cursor>;
        let _gff_write: fn(&mut Cursor, &gff::GffRoot) -> gff::GffResult<()> =
            gff::write_gff_root::<Cursor>;

        let _git_read: fn(&mut Cursor) -> gff::GitResult<gff::GitFile> = gff::read_git::<Cursor>;
        let _git_write: fn(&mut Cursor, &gff::GitFile) -> gff::GitResult<()> =
            gff::write_git::<Cursor>;

        let _key_read: fn(std::path::PathBuf) -> key::KeyResult<key::KeyTable> =
            key::read_key_table_from_file;
        let _key_write: fn(&key::KeyTable, std::path::PathBuf, &str) -> key::KeyResult<()> =
            key::write_key_table_archive;

        let _model_read: fn(&mut Cursor) -> mdl::ModelResult<mdl::Model> =
            mdl::read_model::<Cursor>;
        let _model_write: fn(&mut Cursor, &mdl::Model) -> mdl::ModelResult<()> =
            mdl::write_model::<Cursor>;
        let _ascii_read: fn(&mut Cursor) -> mdl::ModelResult<mdl::AsciiModel> =
            mdl::read_ascii_model::<Cursor>;
        let _ascii_write: fn(&mut Cursor, &mdl::AsciiModel) -> mdl::ModelResult<()> =
            mdl::write_ascii_model::<Cursor>;
        let _parsed_read: fn(&mut Cursor) -> mdl::ModelResult<mdl::ParsedModel> =
            mdl::read_parsed_model::<Cursor>;
        let _parsed_write: fn(&mut Cursor, &mdl::ParsedModel) -> mdl::ModelResult<()> =
            mdl::write_parsed_model::<Cursor>;
        let _binary_read: fn(&mut Cursor) -> mdl::ModelResult<mdl::BinaryModel> =
            mdl::read_binary_model::<Cursor>;
        let _binary_write: fn(&mut Cursor, &mdl::BinaryModel) -> mdl::ModelResult<()> =
            mdl::write_original_binary_model::<Cursor>;
        let _semantic_read: fn(&mut Cursor) -> mdl::ModelResult<mdl::SemanticModel> =
            mdl::read_semantic_model::<Cursor>;
        let _semantic_write: fn(&mut Cursor, &mdl::SemanticModel) -> mdl::ModelResult<()> =
            mdl::write_semantic_model::<Cursor>;
        let _scene_read: fn(&mut Cursor) -> mdl::ModelResult<mdl::NwnScene> =
            mdl::read_scene_model::<Cursor>;
        let _scene_write: fn(&mut Cursor, &mdl::NwnScene) -> mdl::ModelResult<()> =
            mdl::write_scene_model::<Cursor>;

        let _mtr_read: fn(&mut Cursor) -> mtr::MtrResult<mtr::MtrMaterial> =
            mtr::read_mtr::<Cursor>;
        let _mtr_write: fn(&mut Cursor, &mtr::MtrMaterial) -> mtr::MtrResult<()> =
            mtr::write_mtr::<Cursor>;

        let _manifest_read: fn(&mut Cursor) -> nwsync::ManifestResult<nwsync::Manifest> =
            nwsync::read_manifest::<Cursor>;
        let _manifest_write: fn(&mut Cursor, &nwsync::Manifest) -> nwsync::ManifestResult<()> =
            nwsync::write_manifest::<Cursor>;

        let _plt_read: fn(&mut Cursor) -> plt::PltResult<plt::PltTexture> = plt::read_plt::<Cursor>;
        let _plt_write: fn(&mut Cursor, &plt::PltTexture) -> plt::PltResult<()> =
            plt::write_plt::<Cursor>;

        let _set_read: fn(&mut Cursor) -> set::SetResult<set::SetFile> = set::read_set::<Cursor>;
        let _set_write: fn(&mut Cursor, &set::SetFile) -> set::SetResult<()> =
            set::write_set::<Cursor>;

        let _ssf_read: fn(&mut Cursor) -> ssf::SsfResult<ssf::SsfRoot> = ssf::read_ssf::<Cursor>;
        let _ssf_write: fn(&mut Cursor, &ssf::SsfRoot) -> ssf::SsfResult<()> =
            ssf::write_ssf::<Cursor>;

        let _tga_read: fn(&mut Cursor) -> tga::TgaResult<tga::TgaTexture> = tga::read_tga::<Cursor>;
        let _tga_write: fn(&mut Cursor, &tga::TgaTexture) -> std::io::Result<()> =
            tga::write_tga::<Cursor>;

        let _single_tlk_read: fn(Cursor, tlk::CachePolicy) -> tlk::TlkResult<tlk::SingleTlk> =
            tlk::read_single_tlk::<Cursor>;
        let _single_tlk_write: fn(&mut Cursor, &mut tlk::SingleTlk) -> tlk::TlkResult<()> =
            tlk::write_single_tlk::<Cursor>;
        let _tlk_new: fn(Vec<tlk::TlkPair>) -> tlk::Tlk = tlk::Tlk::new;
        let _tlk_write: for<'a> fn(
            &mut [tlk::TlkLayerWriteTarget<'a>],
            &mut tlk::Tlk,
        ) -> tlk::TlkResult<()> = tlk::write_tlk_chain;

        let _twoda_read: fn(Cursor) -> twoda::TwoDaResult<twoda::TwoDa> =
            twoda::read_twoda::<Cursor>;
        let _twoda_write: fn(&mut Cursor, &twoda::TwoDa, bool) -> twoda::TwoDaResult<()> =
            twoda::write_twoda::<Cursor>;

        let _txi_read: fn(&mut dyn std::io::Read) -> txi::TxiResult<txi::TxiFile> = txi::read_txi;
        let _txi_write: fn(&mut Cursor, &txi::TxiFile) -> txi::TxiResult<()> =
            txi::write_txi::<Cursor>;
    }
}
