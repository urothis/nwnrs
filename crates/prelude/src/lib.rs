#![forbid(unsafe_code)]
//! Async client types for the Beamdog NWN masterlist API.
//!
//! This crate models the JSON payloads returned by the public masterlist service and provides
//! a few direct fetch helpers. It is intentionally thin and keeps the response schema close to
//! the wire format.

/// Common imports for consumers of this crate.
pub mod prelude {
    /// Export checksums-related types and traits.
    pub mod checksums {
        pub use nwn_checksums::prelude::*;
    }
    /// Export compressed buffer types and traits.
    pub mod compressedbuf {
        pub use nwn_compressedbuf::prelude::*;
    }
    /// Export core types and traits.
    pub mod core {
        pub use nwn_core::prelude::*;
    }
    /// Export ERF archive types and traits.
    pub mod erf {
        pub use nwn_erf::prelude::*;
    }
    #[cfg(not(target_arch = "wasm32"))]
    /// Export game-related types and traits.
    pub mod game {
        pub use nwn_game::prelude::*;
    }
    /// Export GFF file types and traits.
    pub mod gff {
        pub use nwn_gff::prelude::*;
    }
    /// Export GFF JSON serialization types and traits.
    pub mod gffjson {
        pub use nwn_gffjson::prelude::*;
    }
    /// Export key file types and traits.
    pub mod key {
        pub use nwn_key::prelude::*;
    }
    /// Export LRU cache types and traits.
    pub mod lru {
        pub use nwn_lru::prelude::*;
    }
    /// Export masterlist API client types and traits.
    pub mod masterlist {
        pub use nwn_masterlist::prelude::*;
    }
    /// Export NWN sync client types and traits.
    pub mod nwsync {
        pub use nwn_nwsync::prelude::*;
    }
    /// Export resource directory types and traits.
    pub mod resdir {
        pub use nwn_resdir::prelude::*;
    }
    /// Export resource file types and traits.
    pub mod resfile {
        pub use nwn_resfile::prelude::*;
    }
    /// Export resource manager types and traits.
    pub mod resman {
        pub use nwn_resman::prelude::*;
    }
    /// Export in-memory resource file types and traits.
    pub mod resmemfile {
        pub use nwn_resmemfile::prelude::*;
    }
    /// Export NWSync repository client types and traits.
    pub mod resnwsync {
        pub use nwn_resnwsync::prelude::*;
    }
    /// Export resource reference types and traits.
    pub mod resref {
        pub use nwn_resref::prelude::*;
    }
    /// Export resource type types and traits.
    pub mod restype {
        pub use nwn_restype::prelude::*;
    }
    /// Export structured storage file types and traits.
    pub mod ssf {
        pub use nwn_ssf::prelude::*;
    }
    /// Export stream extension traits.
    pub mod streamext {
        pub use nwn_streamext::prelude::*;
    }
    /// Export TLK file types and traits.
    pub mod tlk {
        pub use nwn_tlk::prelude::*;
    }
    /// Export 2DA file types and traits.
    pub mod twoda {
        pub use nwn_twoda::prelude::*;
    }
    /// Export various utility types and traits.
    pub mod utils {
        pub use nwn_util::prelude::*;
    }
}
