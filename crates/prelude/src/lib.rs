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
    /// Export NWScript compiler and format types and traits.
    pub mod nwscript {
        pub use nwnrs_nwscript::prelude::*;
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
    /// Export structured storage file types and traits.
    pub mod ssf {
        pub use nwnrs_ssf::prelude::*;
    }
    /// Export stream extension traits.
    pub mod streamext {
        pub use nwnrs_streamext::prelude::*;
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
