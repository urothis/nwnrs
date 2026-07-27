//! Authoritative serializable contracts for resource-editor operations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// JSON data retained for inherently dynamic GFF values and metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum JsonValue {
    /// JSON null.
    Null,
    /// JSON boolean.
    Boolean(bool),
    /// Signed JSON integer.
    ///
    /// This variant must precede the floating-point variant so serde keeps
    /// integral GFF values integral across the typed native boundary.
    SignedInteger(i64),
    /// Unsigned JSON integer that does not fit the signed representation.
    UnsignedInteger(u64),
    /// Non-integral JSON number.
    Number(f64),
    /// JSON string.
    String(String),
    /// JSON array.
    Array(Vec<Self>),
    /// JSON object.
    Object(BTreeMap<String, Self>),
}

/// One GFF path component.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum ResourceGffPathSegment {
    /// Field label.
    Field(String),
    /// List index.
    Index(usize),
}

/// Complete editable 2DA table.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceTwoDaTable {
    /// Column names.
    pub columns: Vec<String>,
    /// Optional table default.
    pub default: Option<String>,
    /// Ordered rows.
    pub rows:    Vec<ResourceTwoDaRow>,
}

/// One editable 2DA row.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceTwoDaRow {
    /// Row label.
    pub label: String,
    /// Cells aligned with the table columns.
    pub cells: Vec<Option<String>>,
}

/// One editable TLK entry.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceTlkEntry {
    /// Entry text.
    pub text:            String,
    /// Sound resource reference.
    pub sound_res_ref:   String,
    /// Authored sound length.
    pub sound_length:    f64,
    /// Entry flags.
    pub flags:           u32,
    /// Volume variance.
    pub volume_variance: u32,
    /// Pitch variance.
    pub pitch_variance:  u32,
}

/// Page window attached to pageable resource snapshots.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourcePage {
    /// First row represented by this snapshot.
    pub offset: usize,
    /// Maximum rows requested.
    pub limit:  usize,
}

/// One TLK row, including its string reference.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceTlkSnapshotEntry {
    /// String reference.
    pub str_ref:         u32,
    /// Entry text.
    pub text:            String,
    /// Sound resource reference.
    pub sound_res_ref:   String,
    /// Authored sound length.
    pub sound_length:    f64,
    /// Entry flags.
    pub flags:           i32,
    /// Volume variance.
    pub volume_variance: i32,
    /// Pitch variance.
    pub pitch_variance:  i32,
}

/// Complete editable TLK page.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceTlkData {
    /// Talk-table language id.
    pub language: u32,
    /// Highest populated string reference.
    pub highest:  u32,
    /// Number of matching entries.
    pub total:    usize,
    /// Current page offset.
    pub offset:   usize,
    /// Current page size.
    pub limit:    usize,
    /// Current entries.
    pub entries:  Vec<ResourceTlkSnapshotEntry>,
    /// Generic page metadata.
    pub page:     ResourcePage,
}

/// GFF primitive or container kind.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ResourceGffKind {
    /// Unsigned 8-bit integer.
    Byte,
    /// Signed 8-bit integer.
    Char,
    /// Unsigned 16-bit integer.
    Word,
    /// Signed 16-bit integer.
    Short,
    /// Unsigned 32-bit integer.
    Dword,
    /// Signed 32-bit integer.
    Int,
    /// 32-bit floating-point value.
    Float,
    /// Unsigned 64-bit integer.
    Dword64,
    /// Signed 64-bit integer.
    Int64,
    /// 64-bit floating-point value.
    Double,
    /// Text value.
    String,
    /// Resource reference.
    Resref,
    /// Localized string.
    Locstring,
    /// Opaque byte payload.
    Void,
    /// Nested GFF structure.
    Struct,
    /// List of GFF structures.
    List,
}

/// Summary of a lazily paged GFF child.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ResourceGffChildSummary {
    /// Nested structure.
    Struct {
        /// Exact child path.
        path:  Vec<ResourceGffPathSegment>,
        /// GFF structure id.
        id:    i32,
        /// Number of fields.
        total: usize,
    },
    /// Nested list.
    List {
        /// Exact child path.
        path:  Vec<ResourceGffPathSegment>,
        /// Number of list items.
        total: usize,
    },
}

/// One field in a paged GFF structure.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceGffFieldSummary {
    /// Field label.
    pub label: String,
    /// Exact GFF value kind.
    pub kind:  ResourceGffKind,
    /// Primitive value; containers use null and `child`.
    pub value: JsonValue,
    /// Lazy child descriptor for structs and lists.
    pub child: Option<ResourceGffChildSummary>,
}

/// Paged GFF structure.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceGffStructPage {
    /// Exact node path.
    pub path:   Vec<ResourceGffPathSegment>,
    /// GFF structure id.
    pub id:     i32,
    /// Number of fields.
    pub total:  usize,
    /// Current page offset.
    pub offset: usize,
    /// Current page size.
    pub limit:  usize,
    /// Current fields.
    pub fields: Vec<ResourceGffFieldSummary>,
}

/// One list-item summary.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceGffListItem {
    /// Item index.
    pub index: usize,
    /// Exact item path.
    pub path:  Vec<ResourceGffPathSegment>,
    /// Structure id.
    pub id:    i32,
    /// Number of fields.
    pub total: usize,
}

/// Paged GFF list.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceGffListPage {
    /// Exact node path.
    pub path:   Vec<ResourceGffPathSegment>,
    /// Number of items.
    pub total:  usize,
    /// Current page offset.
    pub offset: usize,
    /// Current page size.
    pub limit:  usize,
    /// Current items.
    pub items:  Vec<ResourceGffListItem>,
}

/// Any GFF page returned by the native editor.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ResourceGffNodePage {
    /// Structure page.
    Struct(ResourceGffStructPage),
    /// List page.
    List(ResourceGffListPage),
}

/// Root GFF snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceGffData {
    /// Four-character file type.
    pub file_type:    String,
    /// GFF file version.
    pub file_version: String,
    /// Root structure page.
    pub root:         ResourceGffNodePage,
    /// Generic page metadata.
    pub page:         ResourcePage,
}

/// One source location recovered from NDB metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceScriptSourceLocation {
    /// NDB source-file identity.
    pub file:      String,
    /// One-based source line.
    pub line:      usize,
    /// Resolved line text when the source is available.
    pub text:      Option<String>,
    /// Whether the source contents were resolved.
    pub available: bool,
}

/// One control-flow successor.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceScriptSuccessor {
    /// Target bytecode offset.
    pub offset: usize,
    /// Control-flow edge kind.
    pub kind:   String,
}

/// One resolved action parameter.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceScriptActionParameter {
    /// Parameter name.
    pub name: String,
    /// NWScript type encoded by the compiler language specification.
    pub ty:   JsonValue,
}

/// Resolved NWScript action metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptAction {
    /// Engine action id.
    pub id:             usize,
    /// Argument count encoded in the instruction.
    pub argument_count: u8,
    /// Action name.
    pub name:           String,
    /// Action return type.
    pub return_type:    JsonValue,
    /// Declared action parameters.
    pub parameters:     Vec<ResourceScriptActionParameter>,
    /// Whether the encoded arity matches the declaration.
    pub arity_matches:  bool,
}

/// One decoded NCS instruction.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptInstruction {
    /// Instruction index in decoded order.
    pub index:            usize,
    /// Byte offset within the code section.
    pub offset:           usize,
    /// Offset relative to the containing function.
    pub local_offset:     Option<usize>,
    /// Encoded instruction size.
    pub size:             usize,
    /// Disassembly label.
    pub label:            Option<String>,
    /// Canonical opcode name.
    pub opcode:           String,
    /// Internal opcode name.
    pub opcode_internal:  String,
    /// Canonical auxiliary-code name.
    pub auxcode:          Option<String>,
    /// Internal auxiliary-code name.
    pub auxcode_internal: String,
    /// Rendered operand.
    pub operand:          String,
    /// Resolved engine action metadata.
    pub action:           Option<ResourceScriptAction>,
    /// Exact encoded bytes as hexadecimal.
    pub raw_hex:          String,
    /// Resolved jump target.
    pub jump_target:      Option<usize>,
    /// Resolved subroutine-call target.
    pub call_target:      Option<usize>,
    /// Control-flow successors.
    pub successors:       Vec<ResourceScriptSuccessor>,
    /// Containing function index.
    pub function_index:   Option<usize>,
    /// NDB source mapping.
    pub source:           Option<ResourceScriptSourceLocation>,
}

/// One basic block in an NCS function.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptBlock {
    /// Inclusive block start.
    pub start:               usize,
    /// Exclusive block end.
    pub end:                 usize,
    /// Decoded instruction indices in the block.
    pub instruction_indices: Vec<usize>,
    /// Outgoing control-flow edges.
    pub successors:          Vec<ResourceScriptSuccessor>,
}

/// One NCS function or synthesized code region.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptFunction {
    /// Function index.
    pub index:       usize,
    /// Function or synthesized region name.
    pub name:        String,
    /// Inclusive bytecode start.
    pub start:       usize,
    /// Exclusive bytecode end.
    pub end:         usize,
    /// Declared return type.
    pub return_type: String,
    /// Declared argument types.
    pub arguments:   Vec<String>,
    /// Whether this region was inferred without NDB metadata.
    pub synthetic:   bool,
    /// Best matching source location.
    pub source:      Option<ResourceScriptSourceLocation>,
    /// Basic blocks.
    pub blocks:      Vec<ResourceScriptBlock>,
}

/// One source file named by NDB metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptSourceFile {
    /// NDB file-table index.
    pub index:     usize,
    /// NDB source identity.
    pub name:      String,
    /// Whether this is the root script.
    pub is_root:   bool,
    /// Whether source contents were resolved.
    pub available: bool,
}

/// One NDB structure field.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceScriptStructField {
    /// Field name.
    pub name:       String,
    /// Field type.
    #[serde(rename = "type")]
    pub field_type: String,
}

/// One NDB structure.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceScriptStruct {
    /// Structure name.
    pub name:   String,
    /// Declared fields.
    pub fields: Vec<ResourceScriptStructField>,
}

/// One NDB variable.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptVariable {
    /// Variable name.
    pub name:           String,
    /// Variable type.
    #[serde(rename = "type")]
    pub variable_type:  String,
    /// Inclusive lifetime start.
    pub start:          u32,
    /// Exclusive lifetime end.
    pub end:            u32,
    /// NDB stack location.
    pub stack_location: i32,
}

/// NDB summary.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptSummary {
    /// NDB source-file count.
    pub files:            usize,
    /// NDB structure count.
    pub structs:          usize,
    /// NDB function count.
    pub functions:        usize,
    /// NDB variable count.
    pub variables:        usize,
    /// NDB line-mapping count.
    pub line_mappings:    usize,
    /// Declared structure details.
    pub struct_entries:   Option<Vec<ResourceScriptStruct>>,
    /// Declared variable details.
    pub variable_entries: Option<Vec<ResourceScriptVariable>>,
}

/// NCS header summary.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptHeader {
    /// Binary format name.
    pub format:            String,
    /// Complete file size.
    pub file_size:         usize,
    /// Size declared by the NCS header.
    pub declared_size:     u32,
    /// Decoded code-section size.
    pub code_size:         usize,
    /// Decoded instruction count.
    pub instruction_count: usize,
}

/// Full NCS/NDB workbench snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceScriptDebugData {
    /// Primary document kind.
    pub primary:      String,
    /// Whether matching NCS bytecode is loaded.
    pub has_ncs:      bool,
    /// Whether NDB metadata is loaded.
    pub has_ndb:      bool,
    /// Whether the NWScript language specification is loaded.
    pub has_langspec: bool,
    /// NDB source-file table.
    pub source_files: Vec<ResourceScriptSourceFile>,
    /// NCS header when bytecode is loaded.
    pub header:       Option<ResourceScriptHeader>,
    /// Aggregate debug metadata.
    pub summary:      ResourceScriptSummary,
    /// Functions and synthesized code regions.
    pub functions:    Vec<ResourceScriptFunction>,
    /// Decoded instructions.
    pub instructions: Vec<ResourceScriptInstruction>,
    /// Workbench diagnostics.
    pub diagnostics:  Vec<String>,
    /// Generic page metadata.
    pub page:         ResourcePage,
}

/// Texture-specific metadata.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum ResourceTextureMetadata {
    /// DDS compression and mip metadata.
    Dds {
        /// DDS pixel format.
        format:     String,
        /// Number of mip levels.
        #[serde(rename = "mipCount")]
        mip_count:  usize,
        /// NWN alpha-mean header value.
        #[serde(rename = "alphaMean")]
        alpha_mean: u8,
    },
    /// TGA storage metadata.
    Tga {
        /// Encoded bits per pixel.
        #[serde(rename = "pixelDepth")]
        pixel_depth:   u8,
        /// TGA image type.
        #[serde(rename = "imageType")]
        image_type:    String,
        /// Whether scanlines are top-to-bottom.
        #[serde(rename = "topToBottom")]
        top_to_bottom: bool,
    },
    /// PLT palette metadata.
    Plt {
        /// Always `PLT`.
        format: String,
    },
}

/// Decoded texture snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceTextureData {
    /// Pixel width.
    pub width:    u32,
    /// Pixel height.
    pub height:   u32,
    /// Format-specific metadata.
    pub metadata: ResourceTextureMetadata,
    /// Generic page metadata.
    pub page:     ResourcePage,
}

/// One archive entry.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceArchiveEntry {
    /// Complete resource filename.
    pub resource:    String,
    /// Resource reference without extension.
    pub resref:      String,
    /// Resource extension.
    pub extension:   String,
    /// Numeric NWN resource type.
    pub type_id:     u16,
    /// Uncompressed byte length.
    pub size:        usize,
    /// Whether the in-memory entry differs from disk.
    pub modified:    bool,
    /// Compression algorithm.
    pub compression: String,
    /// Owning BIF index for KEY resources.
    pub bif_index:   Option<usize>,
    /// Owning BIF filename for KEY resources.
    pub bif:         Option<String>,
}

/// One KEY/BIF file summary.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceBifSummary {
    /// BIF index in the KEY table.
    pub index:       usize,
    /// Recorded BIF filename.
    pub filename:    String,
    /// Recorded drive bitmask.
    pub drives:      u32,
    /// BIF object id.
    pub oid:         u32,
    /// Resource count.
    pub entry_count: usize,
}

/// One ERF localized string.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
pub struct ResourceLocalizedString {
    /// Language identity serialized by the resource library.
    pub language: JsonValue,
    /// Localized text.
    pub text:     String,
}

/// ERF or KEY archive page.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceArchiveData {
    /// Current archive entries.
    pub entries:           Vec<ResourceArchiveEntry>,
    /// Number of matching entries.
    pub total:             usize,
    /// Current page offset.
    pub offset:            usize,
    /// Current page size.
    pub limit:             usize,
    /// Active filter.
    pub query:             String,
    /// KEY/BIF file summaries.
    pub bifs:              Option<Vec<ResourceBifSummary>>,
    /// ERF family file type.
    pub file_type:         Option<String>,
    /// Archive format version.
    pub version:           String,
    /// Archive build year.
    pub build_year:        i32,
    /// Archive build day.
    pub build_day:         i32,
    /// ERF description string reference.
    pub str_ref:           Option<u32>,
    /// Archive object id.
    pub oid:               u32,
    /// ERF localized descriptions.
    pub localized_strings: Option<Vec<ResourceLocalizedString>>,
    /// Generic page metadata.
    pub page:              ResourcePage,
}

/// Common fields shared by every resource snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshotCommon {
    /// Exact physical or immutable virtual path.
    pub path:             String,
    /// Whether normal save is prohibited.
    pub read_only_origin: bool,
    /// Monotonic in-memory edit revision.
    #[cfg_attr(feature = "contract-codegen", ts(type = "number"))]
    pub revision:         u64,
    /// File identity observed when the document was opened or saved.
    pub fingerprint:      ResourceFileFingerprint,
}

/// Typed outer envelope returned for every native resource snapshot.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(tag = "kind")]
pub enum ResourceSnapshot {
    /// Two-dimensional table.
    #[serde(rename = "2da")]
    TwoDa {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Editable table.
        data:   ResourceTwoDaTable,
    },
    /// Talk table.
    #[serde(rename = "tlk")]
    Tlk {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Current talk-table page.
        data:   ResourceTlkData,
    },
    /// Generic file format document.
    #[serde(rename = "gff")]
    Gff {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Root GFF page.
        data:   ResourceGffData,
    },
    /// Compiled NWScript workbench.
    #[serde(rename = "ncs")]
    Ncs {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Disassembly and debug data.
        data:   ResourceScriptDebugData,
    },
    /// NWScript debug-information workbench.
    #[serde(rename = "ndb")]
    Ndb {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Disassembly and debug data.
        data:   ResourceScriptDebugData,
    },
    /// DirectDraw surface texture.
    #[serde(rename = "dds")]
    Dds {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Decoded texture.
        data:   ResourceTextureData,
    },
    /// Truevision texture.
    #[serde(rename = "tga")]
    Tga {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Decoded texture.
        data:   ResourceTextureData,
    },
    /// Palette texture.
    #[serde(rename = "plt")]
    Plt {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Decoded texture.
        data:   ResourceTextureData,
    },
    /// Encapsulated resource archive.
    #[serde(rename = "erf")]
    Erf {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Archive page and metadata.
        data:   ResourceArchiveData,
    },
    /// KEY/BIF archive set.
    #[serde(rename = "key")]
    Key {
        /// Shared document metadata.
        #[serde(flatten)]
        common: ResourceSnapshotCommon,
        /// Archive page and BIF metadata.
        data:   ResourceArchiveData,
    },
}

/// Result of one accepted edit.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceEditResult {
    /// Human-readable undo label.
    pub label:    String,
    /// Exhaustive inverse operation.
    pub inverse:  ResourceEdit,
    /// Updated revision.
    #[cfg_attr(feature = "contract-codegen", ts(type = "number"))]
    pub revision: u64,
    /// Updated snapshot.
    pub snapshot: ResourceSnapshot,
}

/// Result of a binary mutation whose inverse payload remains native.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceBinaryEditResult {
    /// Human-readable undo label.
    pub label:         String,
    /// Single-use opaque token for applying the inverse mutation.
    pub inverse_token: String,
    /// New document revision.
    #[cfg_attr(feature = "contract-codegen", ts(type = "number"))]
    pub revision:      u64,
    /// Complete post-mutation snapshot.
    pub snapshot:      ResourceSnapshot,
}

/// Persisted resource fingerprint returned by save operations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceFileFingerprint {
    /// File byte length.
    #[cfg_attr(feature = "contract-codegen", ts(type = "number"))]
    pub size:           u64,
    /// Modification timestamp in nanoseconds since the Unix epoch.
    #[cfg_attr(feature = "contract-codegen", ts(type = "number"))]
    pub modified_nanos: u64,
    /// Optional SHA-1 content hash.
    pub sha1:           Option<String>,
}

/// Successful resource save.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceSaveResult {
    /// Always true for a successful response.
    pub saved:       bool,
    /// Exact saved path.
    pub path:        String,
    /// New disk fingerprint.
    pub fingerprint: ResourceFileFingerprint,
}

/// One save-as destination that already exists.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceSaveConflict {
    /// Conflicting target.
    pub path:        String,
    /// Current target fingerprint.
    pub fingerprint: ResourceFileFingerprint,
}

/// Preflight result for an atomic save-as operation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceSaveAsPlan {
    /// Complete output set.
    pub targets:            Vec<String>,
    /// Existing targets requiring confirmation.
    pub conflicts:          Vec<ResourceSaveConflict>,
    /// Content-derived confirmation token.
    pub confirmation_token: Option<String>,
}

/// Successful backup write.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceBackupResult {
    /// Always true for a successful response.
    pub backed_up: bool,
    /// Exact backup path.
    pub path:      String,
}

/// Successful document close.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ResourceCloseResult {
    /// Always true for a successful response.
    pub closed: bool,
}

/// Exhaustive resource edit protocol.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-codegen", derive(ts_rs::TS))]
#[serde(tag = "action", rename_all_fields = "camelCase")]
pub enum ResourceEdit {
    /// Replaces a complete GFF root.
    #[serde(rename = "replaceGff")]
    ReplaceGff {
        /// Replacement root.
        root: JsonValue,
    },
    /// Updates one scalar GFF field value.
    #[serde(rename = "setGffValue")]
    SetGffValue {
        /// Owning structure path.
        path:  Vec<ResourceGffPathSegment>,
        /// Field label.
        label: String,
        /// Replacement value.
        value: JsonValue,
    },
    /// Renames one GFF field.
    #[serde(rename = "renameGffField")]
    RenameGffField {
        /// Owning structure path.
        path:      Vec<ResourceGffPathSegment>,
        /// Current label.
        label:     String,
        /// Replacement label.
        new_label: String,
    },
    /// Changes one GFF field type.
    #[serde(rename = "setGffKind")]
    SetGffKind {
        /// Owning structure path.
        path:  Vec<ResourceGffPathSegment>,
        /// Field label.
        label: String,
        /// Replacement GFF kind.
        kind:  String,
        /// Optional replacement value.
        value: Option<JsonValue>,
    },
    /// Adds a GFF field.
    #[serde(rename = "addGffField")]
    AddGffField {
        /// Owning structure path.
        path:  Vec<ResourceGffPathSegment>,
        /// New label.
        label: String,
        /// New GFF kind.
        kind:  String,
        /// Optional initial value.
        value: Option<JsonValue>,
        /// Optional insertion index used by undo.
        index: Option<usize>,
    },
    /// Removes a GFF field.
    #[serde(rename = "removeGffField")]
    RemoveGffField {
        /// Owning structure path.
        path:  Vec<ResourceGffPathSegment>,
        /// Field label.
        label: String,
    },
    /// Adds a structure to a GFF list.
    #[serde(rename = "addGffListStruct")]
    AddGffListStruct {
        /// List path.
        path:  Vec<ResourceGffPathSegment>,
        /// Structure id.
        id:    Option<i32>,
        /// Optional insertion index.
        index: Option<usize>,
        /// Optional complete structure used by undo.
        value: Option<JsonValue>,
    },
    /// Removes a structure from a GFF list.
    #[serde(rename = "removeGffListStruct")]
    RemoveGffListStruct {
        /// List path.
        path:  Vec<ResourceGffPathSegment>,
        /// Removed index.
        index: usize,
    },
    /// Replaces a complete 2DA table.
    #[serde(rename = "replace2da")]
    ReplaceTwoDa {
        /// Replacement table.
        table: ResourceTwoDaTable,
    },
    /// Updates one 2DA cell.
    #[serde(rename = "set2daCell")]
    SetTwoDaCell {
        /// Row index.
        row:    usize,
        /// Column name.
        column: String,
        /// Replacement cell.
        value:  Option<String>,
    },
    /// Updates one 2DA row label.
    #[serde(rename = "set2daRowLabel")]
    SetTwoDaRowLabel {
        /// Row index.
        row:   usize,
        /// Replacement label.
        label: String,
    },
    /// Updates or removes one TLK entry.
    #[serde(rename = "setTlkEntry")]
    SetTlkEntry {
        /// String reference.
        str_ref: u32,
        /// Replacement entry, or null to remove it.
        entry:   Option<ResourceTlkEntry>,
    },
    /// Clears one TLK override.
    #[serde(rename = "clearTlkOverride")]
    ClearTlkOverride {
        /// String reference.
        str_ref: u32,
    },
    /// Changes the TLK language id.
    #[serde(rename = "setTlkLanguage")]
    SetTlkLanguage {
        /// Language id.
        language: u32,
    },
    /// Updates one PLT pixel.
    #[serde(rename = "setPltPixel")]
    SetPltPixel {
        /// X coordinate.
        x:     u32,
        /// Y coordinate.
        y:     u32,
        /// Palette value.
        value: u8,
        /// Material layer.
        layer: u8,
    },
    /// Renames an archive entry.
    #[serde(rename = "renameEntry")]
    RenameEntry {
        /// Current resource name.
        resource:     String,
        /// Replacement resource name.
        new_resource: String,
        /// Optional KEY/BIF owner.
        bif_index:    Option<usize>,
    },
}
