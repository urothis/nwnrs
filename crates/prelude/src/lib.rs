#![forbid(unsafe_code)]
//! Async client types for the Beamdog NWN masterlist API.
//!
//! This crate models the JSON payloads returned by the public masterlist
//! service and provides a few direct fetch helpers. It is intentionally thin
//! and keeps the response schema close to the wire format.

/// Common imports for consumers of this crate.
pub mod prelude {
    /// Export checksums-related types and traits.
    pub mod checksums {
        pub use nwnrs_checksums::prelude::*;
    }
    /// Export compressed buffer types and traits.
    pub mod compressedbuf {
        pub use nwnrs_compressedbuf::prelude::*;
    }
    /// Export core types and traits.
    pub mod core {
        pub use nwnrs_core::prelude::*;
    }
    /// Export DDS texture types and traits.
    pub mod dds {
        pub use nwnrs_dds::prelude::*;
    }
    /// Export ERF archive types and traits.
    pub mod erf {
        pub use nwnrs_erf::prelude::*;
    }
    /// Export EXO file types and traits.
    pub mod exo {
        pub use nwnrs_exo::prelude::*;
    }
    #[cfg(not(target_arch = "wasm32"))]
    /// Export game-related types and traits.
    pub mod game {
        pub use nwnrs_game::prelude::*;
    }
    /// Export GFF file types and traits.
    pub mod gff {
        pub use nwnrs_gff::prelude::*;
    }
    /// Export key file types and traits.
    pub mod key {
        pub use nwnrs_key::prelude::*;
    }
    /// Export LRU cache types and traits.
    pub mod lru {
        pub use nwnrs_lru::prelude::*;
    }
    /// Export masterlist API client types and traits.
    pub mod masterlist {
        pub use nwnrs_masterlist::prelude::*;
    }
    /// Export NWN MDL types and traits.
    pub mod mdl {
        pub use nwnrs_mdl::prelude::*;
    }
    /// Export MTR material types and traits.
    pub mod mtr {
        pub use nwnrs_mtr::prelude::*;
    }
    /// Export NWScript compiler and format types and traits.
    pub mod nwscript {
        pub use nwnrs_nwscript::prelude::*;
    }
    /// Export PLT texture types and traits.
    pub mod plt {
        pub use nwnrs_plt::prelude::*;
    }
    /// Export NWN sync client types and traits.
    pub mod nwsync {
        pub use nwnrs_nwsync::prelude::*;
    }
    /// Export resource directory types and traits.
    pub mod resdir {
        pub use nwnrs_resdir::prelude::*;
    }
    /// Export resource file types and traits.
    pub mod resfile {
        pub use nwnrs_resfile::prelude::*;
    }
    /// Export resource manager types and traits.
    pub mod resman {
        pub use nwnrs_resman::prelude::*;
    }
    /// Export in-memory resource file types and traits.
    pub mod resmemfile {
        pub use nwnrs_resmemfile::prelude::*;
    }
    /// Export NWSync repository client types and traits.
    pub mod resnwsync {
        pub use nwnrs_resnwsync::prelude::*;
    }
    /// Export resource reference types and traits.
    pub mod resref {
        pub use nwnrs_resref::prelude::*;
    }
    /// Export resource type types and traits.
    pub mod restype {
        pub use nwnrs_restype::prelude::*;
    }
    /// Export tileset `SET` types and traits.
    pub mod set {
        pub use nwnrs_set::prelude::*;
    }
    /// Export structured storage file types and traits.
    pub mod ssf {
        pub use nwnrs_ssf::prelude::*;
    }
    /// Export stream extension traits.
    pub mod streamext {
        pub use nwnrs_streamext::prelude::*;
    }
    /// Export TGA texture types and traits.
    pub mod tga {
        pub use nwnrs_tga::prelude::*;
    }
    /// Export TLK file types and traits.
    pub mod tlk {
        pub use nwnrs_tlk::prelude::*;
    }
    /// Export 2DA file types and traits.
    pub mod twoda {
        pub use nwnrs_twoda::prelude::*;
    }
    /// Export various utility types and traits.
    pub mod utils {
        pub use nwnrs_util::prelude::*;
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::prelude;

    #[test]
    fn prelude_modules_expose_common_workspace_types() {
        let digest = prelude::checksums::secure_hash(b"abc");
        assert_eq!(
            digest.to_string(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );

        let language = prelude::core::resolve_language("en").unwrap_or_else(|error| {
            panic!("resolve language: {error}");
        });
        assert_eq!(language, prelude::core::Language::English);

        let res_type = prelude::restype::get_res_type("2da");
        let rr = prelude::resref::new_res_ref("table", res_type).unwrap_or_else(|error| {
            panic!("new rr: {error}");
        });
        assert_eq!(rr.to_string(), "table.2da");

        let mut cache = prelude::lru::WeightedLru::new(2, 1);
        cache.insert("k", 1);
        assert_eq!(cache.get(&"k"), Some(&1));
        let mdl = prelude::mdl::Model::from_text("newmodel a");
        assert_eq!(mdl.as_text().unwrap_or(""), "newmodel a");
        assert_eq!(prelude::plt::PLT_RES_TYPE.0, 6);
        assert_eq!(prelude::dds::DdsFormat::Dxt1.bytes_per_block(), 8);
        assert_eq!(
            prelude::exo::ExoResFileCompressionType::from_u32(1),
            Some(prelude::exo::ExoResFileCompressionType::CompressedBuf)
        );
    }
}
