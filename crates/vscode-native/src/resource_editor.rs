use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use napi::{
    Task,
    bindgen_prelude::{AsyncTask, Buffer},
};
use napi_derive::napi;
use nwnrs_nwscript::{
    LangSpec, NCS_BINARY_HEADER_SIZE, NcsDisassemblyOptions, NcsInstruction, NcsOpcode, Ndb,
    decode_ncs_header, decode_ncs_instructions, disassemble_ncs, parse_langspec_bytes, read_ndb,
};
use nwnrs_types::{
    checksums::sha1_digest,
    compressedbuf::Algorithm,
    dds::{DdsFormat, DdsTexture, read_dds, write_dds},
    erf::{Erf, ErfVersion, ErfWriteOptions, read_erf, write_erf_with_options},
    exo::ExoResFileCompressionType,
    gff::{
        GffCExoLocString, GffField, GffRoot, GffStruct, GffValue, gff_root_from_json_bytes,
        gff_root_to_json_bytes, read_gff_root, write_gff_root,
    },
    key::{KeyBifEntry, KeyBifVersion, KeyTable, read_key_table_from_file, write_key_and_bif},
    localization::Language,
    plt::{PltPixel, PltRenderSpec, PltTexture, read_plt, write_plt},
    resman::{CachePolicy, Res, ResContainer, ResRef, ResolvedResRef, get_res_ext},
    tga::{TgaTexture, read_tga, write_tga},
    tlk::{SingleTlk, TlkEntry, read_single_tlk, write_single_tlk},
    twoda::{TwoDa, read_twoda, write_twoda},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DEFAULT_PAGE_SIZE: usize = 200;
const MAX_PAGE_SIZE: usize = 2_000;

type EditorResult<T> = Result<T, String>;

#[derive(Default)]
struct ResourceEditorState {
    documents: Mutex<HashMap<String, EditorDocument>>,
}

/// Persistent native resource editor used by the VS Code custom editors.
#[napi]
pub struct ResourceEditorService {
    state: Arc<ResourceEditorState>,
}

#[napi]
impl ResourceEditorService {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            state: Arc::new(ResourceEditorState::default()),
        }
    }

    /// Executes a typed resource-editor request away from the JavaScript event
    /// loop.
    #[napi]
    pub fn execute(&self, method: String, request_json: String) -> AsyncTask<ResourceEditorTask> {
        AsyncTask::new(ResourceEditorTask {
            state: Arc::clone(&self.state),
            method,
            request_json,
        })
    }

    /// Reads an archive entry as a native byte buffer. Large model and texture
    /// payloads must not be expanded through JSON/base64 on their way to a
    /// renderer worker.
    #[napi]
    pub fn read_entry_bytes(
        &self,
        document_id: String,
        resource: String,
    ) -> AsyncTask<ResourceEntryReadTask> {
        AsyncTask::new(ResourceEntryReadTask {
            state: Arc::clone(&self.state),
            document_id,
            resource,
        })
    }
}

impl Default for ResourceEditorService {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ResourceEditorTask {
    state:        Arc<ResourceEditorState>,
    method:       String,
    request_json: String,
}

pub struct ResourceEntryReadTask {
    state:       Arc<ResourceEditorState>,
    document_id: String,
    resource:    String,
}

impl Task for ResourceEntryReadTask {
    type JsValue = Buffer;
    type Output = Vec<u8>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let documents = self.state.documents.lock().map_err(|error| {
            napi::Error::from_reason(format!("resource document map is poisoned: {error}"))
        })?;
        let document = documents.get(&self.document_id).ok_or_else(|| {
            napi::Error::from_reason(format!(
                "resource document is not open: {}",
                self.document_id
            ))
        })?;
        document
            .content
            .read_entry(&self.resource)
            .map_err(napi::Error::from_reason)
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

impl Task for ResourceEditorTask {
    type JsValue = String;
    type Output = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: Value = serde_json::from_str(&self.request_json).map_err(|error| {
            napi::Error::from_reason(format!("invalid editor request: {error}"))
        })?;
        let response = execute_request(&self.state, &self.method, request)
            .map_err(napi::Error::from_reason)?;
        serde_json::to_string(&response).map_err(|error| {
            napi::Error::from_reason(format!("failed to encode response: {error}"))
        })
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

fn execute_request(
    state: &ResourceEditorState,
    method: &str,
    request: Value,
) -> EditorResult<Value> {
    match method {
        "openDocument" => open_document(state, request),
        "openDocumentBytes" => open_document_bytes(state, request),
        "snapshot" => with_document_mut(state, &request, |document| document.snapshot(&request)),
        "configureScriptDebug" => with_document_mut(state, &request, |document| {
            document.configure_script_debug(&request)?;
            document.snapshot(&request)
        }),
        "applyEdit" => with_document_mut(state, &request, |document| document.apply_edit(&request)),
        "readEntry" => with_document(state, &request, |document| document.read_entry(&request)),
        "exportDocument" => with_document_mut(state, &request, |document| {
            Ok(json!({ "contents": BASE64.encode(document.content.serialize()?) }))
        }),
        "saveDocument" => with_document_mut(state, &request, |document| document.save(&request)),
        "saveDocumentAs" => {
            with_document_mut(state, &request, |document| document.save_as(&request))
        }
        "backupDocument" => {
            with_document_mut(state, &request, |document| document.backup(&request))
        }
        "revertDocument" => {
            with_document_mut(state, &request, |document| document.revert(&request))
        }
        "closeDocument" => close_document(state, &request),
        _ => Err(format!("resource editor does not export {method}")),
    }
}

fn request_id(request: &Value) -> EditorResult<&str> {
    request
        .get("documentId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "documentId is required".to_string())
}

fn with_document(
    state: &ResourceEditorState,
    request: &Value,
    operation: impl FnOnce(&EditorDocument) -> EditorResult<Value>,
) -> EditorResult<Value> {
    let id = request_id(request)?;
    let documents = state
        .documents
        .lock()
        .map_err(|error| format!("resource document map is poisoned: {error}"))?;
    let document = documents
        .get(id)
        .ok_or_else(|| format!("resource document is not open: {id}"))?;
    operation(document)
}

fn with_document_mut(
    state: &ResourceEditorState,
    request: &Value,
    operation: impl FnOnce(&mut EditorDocument) -> EditorResult<Value>,
) -> EditorResult<Value> {
    let id = request_id(request)?;
    let mut documents = state
        .documents
        .lock()
        .map_err(|error| format!("resource document map is poisoned: {error}"))?;
    let document = documents
        .get_mut(id)
        .ok_or_else(|| format!("resource document is not open: {id}"))?;
    operation(document)
}

fn close_document(state: &ResourceEditorState, request: &Value) -> EditorResult<Value> {
    let id = request_id(request)?;
    let mut documents = state
        .documents
        .lock()
        .map_err(|error| format!("resource document map is poisoned: {error}"))?;
    documents.remove(id);
    Ok(json!({ "closed": true }))
}

fn open_document(state: &ResourceEditorState, request: Value) -> EditorResult<Value> {
    let id = request_id(&request)?.to_string();
    let path = required_path(&request, "path")?;
    let read_only_origin = request
        .get("readOnlyOrigin")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let backup_path = request
        .get("backupPath")
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let mut document = if let Some(backup_path) = backup_path {
        EditorDocument::from_backup(path, backup_path, read_only_origin)?
    } else {
        EditorDocument::open(path, read_only_origin)?
    };
    let response = document.snapshot(&request)?;
    let mut documents = state
        .documents
        .lock()
        .map_err(|error| format!("resource document map is poisoned: {error}"))?;
    documents.insert(id, document);
    Ok(response)
}

fn open_document_bytes(state: &ResourceEditorState, request: Value) -> EditorResult<Value> {
    let id = request_id(&request)?.to_string();
    let path = required_path(&request, "path")?;
    let bytes = BASE64
        .decode(required_string(&request, "contents")?)
        .map_err(|error| format!("invalid resource payload: {error}"))?;
    let content = EditorContent::parse(&path, &bytes)?;
    let fingerprint = FileFingerprint {
        size:           u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        modified_nanos: 0,
        sha1:           sha1_digest(&bytes).to_string(),
    };
    let mut document = EditorDocument {
        path,
        read_only_origin: true,
        fingerprint,
        related_fingerprints: BTreeMap::new(),
        revision: 0,
        content,
    };
    let response = document.snapshot(&request)?;
    let mut documents = state
        .documents
        .lock()
        .map_err(|error| format!("resource document map is poisoned: {error}"))?;
    documents.insert(id, document);
    Ok(response)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FileFingerprint {
    size:           u64,
    modified_nanos: u128,
    sha1:           String,
}

impl FileFingerprint {
    fn read(path: &Path) -> EditorResult<Self> {
        let bytes = fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let metadata = fs::metadata(path)
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        let modified_nanos = metadata
            .modified()
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Ok(Self {
            size: metadata.len(),
            modified_nanos,
            sha1: sha1_digest(bytes).to_string(),
        })
    }
}

struct EditorDocument {
    path:                 PathBuf,
    read_only_origin:     bool,
    fingerprint:          FileFingerprint,
    related_fingerprints: BTreeMap<PathBuf, FileFingerprint>,
    revision:             u64,
    content:              EditorContent,
}

enum EditorContent {
    Gff(GffDocument),
    TwoDa(TwoDa),
    Tlk(SingleTlk),
    Dds(DdsTexture),
    Tga(TgaTexture),
    Plt(PltTexture),
    Erf(ErfDocument),
    Key(KeyDocument),
    ScriptDebug(ScriptDebugDocument),
}

#[derive(Clone, Copy)]
enum GffSourceEncoding {
    Binary,
    Json,
}

struct GffDocument {
    root:     GffRoot,
    encoding: GffSourceEncoding,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScriptDebugPrimary {
    Ncs,
    Ndb,
}

struct ScriptDebugDocument {
    primary:     ScriptDebugPrimary,
    primary_raw: Vec<u8>,
    ncs:         Option<Vec<u8>>,
    ndb:         Option<Ndb>,
    langspec:    Option<LangSpec>,
    sources:     BTreeMap<String, Vec<String>>,
}

impl EditorDocument {
    fn open(path: PathBuf, read_only_origin: bool) -> EditorResult<Self> {
        let bytes = fs::read(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let fingerprint = FileFingerprint::read(&path)?;
        let content = EditorContent::parse(&path, &bytes)?;
        let related_fingerprints = content.related_fingerprints(&path)?;
        Ok(Self {
            path,
            read_only_origin,
            fingerprint,
            related_fingerprints,
            revision: 0,
            content,
        })
    }

    fn from_backup(
        path: PathBuf,
        backup_path: PathBuf,
        read_only_origin: bool,
    ) -> EditorResult<Self> {
        let envelope_bytes = fs::read(&backup_path)
            .map_err(|error| format!("failed to read backup {}: {error}", backup_path.display()))?;
        let envelope: BackupEnvelope = serde_json::from_slice(&envelope_bytes)
            .map_err(|error| format!("invalid nwnrs resource backup: {error}"))?;
        let bytes = BASE64
            .decode(envelope.contents)
            .map_err(|error| format!("invalid resource backup payload: {error}"))?;
        let fingerprint = envelope.fingerprint;
        let content = EditorContent::parse_backup(&path, &bytes)?;
        Ok(Self {
            path,
            read_only_origin,
            fingerprint,
            related_fingerprints: envelope.related_fingerprints,
            revision: envelope.revision,
            content,
        })
    }

    fn snapshot(&mut self, request: &Value) -> EditorResult<Value> {
        let data = self.content.snapshot(request)?;
        Ok(json!({
            "path": self.path,
            "kind": self.content.kind(),
            "readOnlyOrigin": self.read_only_origin,
            "revision": self.revision,
            "fingerprint": self.fingerprint,
            "data": data,
        }))
    }

    fn configure_script_debug(&mut self, request: &Value) -> EditorResult<()> {
        match &mut self.content {
            EditorContent::ScriptDebug(value) => value.configure(request),
            _ => Err(format!(
                "{} is not an NCS/NDB document",
                self.content.kind()
            )),
        }
    }

    fn apply_edit(&mut self, request: &Value) -> EditorResult<Value> {
        let edit = request
            .get("edit")
            .ok_or_else(|| "edit is required".to_string())?;
        let (label, inverse) = self.content.apply_edit(edit)?;
        self.revision = self.revision.saturating_add(1);
        let snapshot = self.snapshot(request)?;
        Ok(json!({
            "label": label,
            "inverse": inverse,
            "revision": self.revision,
            "snapshot": snapshot,
        }))
    }

    fn read_entry(&self, request: &Value) -> EditorResult<Value> {
        let resource = required_string(request, "resource")?;
        let bytes = self.content.read_entry(resource)?;
        Ok(json!({ "resource": resource, "contents": BASE64.encode(bytes) }))
    }

    fn save(&mut self, request: &Value) -> EditorResult<Value> {
        if self.read_only_origin {
            return Err(
                "READ_ONLY_ORIGIN: installed game resources must be saved as an override"
                    .to_string(),
            );
        }
        let force = request
            .get("force")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !force {
            if self.path.exists() && FileFingerprint::read(&self.path)? != self.fingerprint {
                return Err(
                    "EXTERNAL_CHANGE: the file changed on disk after it was opened".to_string(),
                );
            }
            for (path, fingerprint) in &self.related_fingerprints {
                if !path.exists() || FileFingerprint::read(path)? != *fingerprint {
                    return Err(format!(
                        "EXTERNAL_CHANGE: {} changed on disk after the KEY table was opened",
                        path.display()
                    ));
                }
            }
        }
        self.persist_to(self.path.clone())?;
        Ok(json!({ "saved": true, "path": self.path, "fingerprint": self.fingerprint }))
    }

    fn save_as(&mut self, request: &Value) -> EditorResult<Value> {
        let path = required_path(request, "path")?;
        self.persist_to(path.clone())?;
        self.path = path;
        self.read_only_origin = false;
        Ok(json!({ "saved": true, "path": self.path, "fingerprint": self.fingerprint }))
    }

    fn persist_to(&mut self, path: PathBuf) -> EditorResult<()> {
        match &mut self.content {
            EditorContent::Key(key) => key.write_atomic(&path)?,
            _ => atomic_write(&path, &self.content.serialize_for_path(&path)?)?,
        }
        self.content.update_path_encoding(&path)?;
        self.fingerprint = FileFingerprint::read(&path)?;
        self.related_fingerprints = self.content.related_fingerprints(&path)?;
        Ok(())
    }

    fn backup(&mut self, request: &Value) -> EditorResult<Value> {
        let path = required_path(request, "path")?;
        let bytes = self.content.serialize_for_backup()?;
        let envelope = BackupEnvelope {
            revision:             self.revision,
            fingerprint:          self.fingerprint.clone(),
            related_fingerprints: self.related_fingerprints.clone(),
            contents:             BASE64.encode(bytes),
        };
        let encoded = serde_json::to_vec(&envelope)
            .map_err(|error| format!("failed to encode resource backup: {error}"))?;
        atomic_write(&path, &encoded)?;
        Ok(json!({ "backedUp": true, "path": path }))
    }

    fn revert(&mut self, request: &Value) -> EditorResult<Value> {
        let replacement = Self::open(self.path.clone(), self.read_only_origin)?;
        self.fingerprint = replacement.fingerprint;
        self.related_fingerprints = replacement.related_fingerprints;
        self.content = replacement.content;
        self.revision = self.revision.saturating_add(1);
        self.snapshot(request)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupEnvelope {
    revision:             u64,
    fingerprint:          FileFingerprint,
    #[serde(default)]
    related_fingerprints: BTreeMap<PathBuf, FileFingerprint>,
    contents:             String,
}

impl EditorContent {
    fn parse(path: &Path, bytes: &[u8]) -> EditorResult<Self> {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut cursor = Cursor::new(bytes.to_vec());
        if let Some(encoding) = gff_source_encoding(path) {
            let root = match encoding {
                GffSourceEncoding::Binary => read_gff_root(&mut cursor).map_err(display_error)?,
                GffSourceEncoding::Json => {
                    gff_root_from_json_bytes(bytes).map_err(display_error)?
                }
            };
            return Ok(Self::Gff(GffDocument {
                root,
                encoding,
            }));
        }
        match extension.as_str() {
            "2da" => read_twoda(&mut cursor)
                .map(Self::TwoDa)
                .map_err(display_error),
            "tlk" => read_single_tlk(cursor, CachePolicy::Use)
                .map(Self::Tlk)
                .map_err(display_error),
            "dds" => read_dds(&mut cursor).map(Self::Dds).map_err(display_error),
            "tga" => read_tga(&mut cursor).map(Self::Tga).map_err(display_error),
            "plt" => read_plt(&mut cursor).map(Self::Plt).map_err(display_error),
            "erf" | "hak" | "mod" | "nwm" => read_erf(cursor, path.display().to_string())
                .and_then(ErfDocument::new)
                .map(Self::Erf)
                .map_err(display_error),
            "key" => read_key_table_from_file(path)
                .and_then(KeyDocument::new)
                .map(Self::Key)
                .map_err(display_error),
            "ncs" => ScriptDebugDocument::from_ncs(bytes).map(Self::ScriptDebug),
            "ndb" => ScriptDebugDocument::from_ndb(bytes).map(Self::ScriptDebug),
            _ => Err(format!(
                "unsupported nwnrs resource type: {}",
                path.display()
            )),
        }
    }

    fn parse_backup(path: &Path, bytes: &[u8]) -> EditorResult<Self> {
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("key"))
        {
            let bundle: Value = serde_json::from_slice(bytes)
                .map_err(|error| format!("invalid KEY/BIF backup bundle: {error}"))?;
            if bundle.get("kind").and_then(Value::as_str) != Some("keyBundle") {
                return Err("invalid KEY/BIF backup bundle kind".to_string());
            }
            let temporary = std::env::temp_dir().join(format!(
                "nwnrs-key-restore-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir(&temporary).map_err(display_error)?;
            let result = (|| -> EditorResult<Self> {
                let files = bundle
                    .get("files")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "KEY/BIF backup has no files".to_string())?;
                for file in files {
                    let relative = PathBuf::from(required_string(file, "path")?);
                    if relative.is_absolute()
                        || relative
                            .components()
                            .any(|component| matches!(component, std::path::Component::ParentDir))
                    {
                        return Err("KEY/BIF backup contains an unsafe path".to_string());
                    }
                    let destination = temporary.join(&relative);
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent).map_err(display_error)?;
                    }
                    let contents = BASE64
                        .decode(required_string(file, "contents")?)
                        .map_err(display_error)?;
                    fs::write(&destination, contents).map_err(display_error)?;
                }
                let key_path = temporary.join("backup.key");
                let table = read_key_table_from_file(&key_path).map_err(display_error)?;
                let mut document = KeyDocument::new(table).map_err(display_error)?;
                document.materialize()?;
                Ok(Self::Key(document))
            })();
            let _ = fs::remove_dir_all(&temporary);
            return result;
        }
        Self::parse(path, bytes)
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Gff(_) => "gff",
            Self::TwoDa(_) => "2da",
            Self::Tlk(_) => "tlk",
            Self::Dds(_) => "dds",
            Self::Tga(_) => "tga",
            Self::Plt(_) => "plt",
            Self::Erf(_) => "erf",
            Self::Key(_) => "key",
            Self::ScriptDebug(value) => match value.primary {
                ScriptDebugPrimary::Ncs => "ncs",
                ScriptDebugPrimary::Ndb => "ndb",
            },
        }
    }

    fn related_fingerprints(
        &self,
        path: &Path,
    ) -> EditorResult<BTreeMap<PathBuf, FileFingerprint>> {
        let mut result = BTreeMap::new();
        if let Self::Key(value) = self {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            for bif in &value.bifs {
                let Some(filename) = bif.recorded_filename.as_deref() else {
                    continue;
                };
                let bif_path = parent.join(filename.replace('\\', "/"));
                if bif_path.exists() {
                    result.insert(bif_path.clone(), FileFingerprint::read(&bif_path)?);
                }
            }
        }
        Ok(result)
    }

    fn snapshot(&mut self, request: &Value) -> EditorResult<Value> {
        match self {
            Self::Gff(value) => Ok(gff_root_to_json(&value.root)),
            Self::TwoDa(value) => Ok(twoda_to_json(value)),
            Self::Tlk(value) => Ok(tlk_snapshot(value, request)?),
            Self::Dds(value) => texture_snapshot(
                value.width,
                value.height,
                &value.decode_rgba8().map_err(display_error)?,
                json!({
                    "format": match value.format { DdsFormat::Dxt1 => "DXT1", DdsFormat::Dxt5 => "DXT5" },
                    "mipCount": value.mip_count(),
                    "alphaMean": value.nwn_header.alpha_mean,
                }),
            ),
            Self::Tga(value) => texture_snapshot(
                u32::from(value.width),
                u32::from(value.height),
                &value.decode_rgba8().map_err(display_error)?,
                json!({
                    "pixelDepth": value.pixel_depth,
                    "imageType": format!("{:?}", value.image_type),
                    "topToBottom": value.top_to_bottom(),
                }),
            ),
            Self::Plt(value) => {
                let rgba = value.render_rgba8(&PltRenderSpec::default()).map_err(display_error)?;
                let pixel_data = value
                    .pixels
                    .iter()
                    .flat_map(|pixel| [pixel.value, pixel.layer_id])
                    .collect::<Vec<_>>();
                texture_snapshot(
                    value.width,
                    value.height,
                    &rgba,
                    json!({ "pixelData": BASE64.encode(pixel_data), "format": "PLT" }),
                )
            }
            Self::Erf(value) => Ok(value.snapshot(request)),
            Self::Key(value) => Ok(value.snapshot(request)),
            Self::ScriptDebug(value) => value.snapshot(),
        }
        .map(|mut snapshot| {
            if let Some(object) = snapshot.as_object_mut() {
                object.insert(
                    "page".to_string(),
                    json!({
                        "offset": request.get("offset").and_then(Value::as_u64).unwrap_or(0),
                        "limit": page_limit(request),
                    }),
                );
            }
            snapshot
        })
    }

    fn apply_edit(&mut self, edit: &Value) -> EditorResult<(String, Value)> {
        let action = required_string(edit, "action")?;
        match self {
            Self::Gff(value) if action == "replaceGff" => {
                let before = gff_root_to_json(&value.root);
                let root = edit
                    .get("root")
                    .ok_or_else(|| "root is required".to_string())?;
                let mut candidate = value.root.clone();
                merge_gff_root(&mut candidate, root)?;
                value.root = candidate;
                Ok((
                    "Edit GFF".to_string(),
                    json!({ "action": "replaceGff", "root": before }),
                ))
            }
            Self::TwoDa(value) => apply_twoda_edit(value, action, edit),
            Self::Tlk(value) => apply_tlk_edit(value, action, edit),
            Self::Dds(value) if action == "replaceTexture" => {
                let inverse = encoded_texture_inverse(write_dds_bytes(value)?);
                let (width, height, rgba) = decode_texture_edit(edit)?;
                *value = DdsTexture::encode_rgba8(width, height, value.format, &rgba)
                    .map_err(display_error)?;
                Ok(("Replace DDS pixels".to_string(), inverse))
            }
            Self::Dds(value) if action == "restoreTextureBytes" => {
                let inverse = encoded_texture_inverse(write_dds_bytes(value)?);
                let bytes = decode_encoded_texture(edit)?;
                *value = read_dds(&mut Cursor::new(bytes)).map_err(display_error)?;
                Ok(("Restore DDS texture".to_string(), inverse))
            }
            Self::Tga(value) if action == "replaceTexture" => {
                let inverse = encoded_texture_inverse(write_tga_bytes(value)?);
                let (width, height, rgba) = decode_texture_edit(edit)?;
                *value = TgaTexture::encode_rgba8(
                    u16::try_from(width)
                        .map_err(|error| format!("TGA width exceeds 65535: {error}"))?,
                    u16::try_from(height)
                        .map_err(|error| format!("TGA height exceeds 65535: {error}"))?,
                    &rgba,
                )
                .map_err(display_error)?;
                Ok(("Replace TGA pixels".to_string(), inverse))
            }
            Self::Tga(value) if action == "restoreTextureBytes" => {
                let inverse = encoded_texture_inverse(write_tga_bytes(value)?);
                let bytes = decode_encoded_texture(edit)?;
                *value = read_tga(&mut Cursor::new(bytes)).map_err(display_error)?;
                Ok(("Restore TGA texture".to_string(), inverse))
            }
            Self::Plt(value) if action == "setPltPixel" => apply_plt_edit(value, edit),
            Self::Erf(value) => value.apply_edit(action, edit),
            Self::Key(value) => value.apply_edit(action, edit),
            Self::ScriptDebug(_) => Err("NCS/NDB workbench documents are read-only".to_string()),
            _ => Err(format!("edit {action} is not valid for {}", self.kind())),
        }
    }

    fn read_entry(&self, resource: &str) -> EditorResult<Vec<u8>> {
        match self {
            Self::Erf(value) => value.read_entry(resource),
            Self::Key(value) => value.read_entry(resource),
            _ => Err(format!("{} is not an archive", self.kind())),
        }
    }

    fn serialize(&mut self) -> EditorResult<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        match self {
            Self::Gff(value) => match value.encoding {
                GffSourceEncoding::Binary => {
                    write_gff_root(&mut cursor, &value.root).map_err(display_error)?
                }
                GffSourceEncoding::Json => {
                    return gff_root_to_json_bytes(&value.root).map_err(display_error)
                }
            },
            Self::TwoDa(value) => write_twoda(&mut cursor, value, false).map_err(display_error)?,
            Self::Tlk(value) => write_single_tlk(&mut cursor, value).map_err(display_error)?,
            Self::Dds(value) => write_dds(&mut cursor, value).map_err(display_error)?,
            Self::Tga(value) => write_tga(&mut cursor, value).map_err(display_error)?,
            Self::Plt(value) => write_plt(&mut cursor, value).map_err(display_error)?,
            Self::Erf(value) => value.write(&mut cursor)?,
            Self::Key(_) => {
                return Err("KEY/BIF sets require transactional path serialization".to_string())
            }
            Self::ScriptDebug(value) => return Ok(value.primary_raw.clone()),
        }
        Ok(cursor.into_inner())
    }

    fn serialize_for_path(&mut self, path: &Path) -> EditorResult<Vec<u8>> {
        match self {
            Self::Gff(value) => match gff_source_encoding(path)
                .ok_or_else(|| format!("{} is not a supported GFF destination", path.display()))?
            {
                GffSourceEncoding::Binary => {
                    let mut cursor = Cursor::new(Vec::new());
                    write_gff_root(&mut cursor, &value.root).map_err(display_error)?;
                    Ok(cursor.into_inner())
                }
                GffSourceEncoding::Json => {
                    gff_root_to_json_bytes(&value.root).map_err(display_error)
                }
            },
            _ if gff_source_encoding(path).is_some() => Err(format!(
                "cannot save {} content as GFF source {}",
                self.kind(),
                path.display()
            )),
            _ => self.serialize(),
        }
    }

    fn update_path_encoding(&mut self, path: &Path) -> EditorResult<()> {
        if let Self::Gff(value) = self {
            value.encoding = gff_source_encoding(path)
                .ok_or_else(|| format!("{} is not a supported GFF destination", path.display()))?;
        }
        Ok(())
    }

    fn serialize_for_backup(&mut self) -> EditorResult<Vec<u8>> {
        match self {
            Self::Key(value) => value.backup_bytes(),
            _ => self.serialize(),
        }
    }
}

impl ScriptDebugDocument {
    fn from_ncs(bytes: &[u8]) -> EditorResult<Self> {
        decode_ncs_header(bytes).map_err(display_error)?;
        decode_ncs_instructions(bytes).map_err(display_error)?;
        Ok(Self {
            primary:     ScriptDebugPrimary::Ncs,
            primary_raw: bytes.to_vec(),
            ncs:         Some(bytes.to_vec()),
            ndb:         None,
            langspec:    None,
            sources:     BTreeMap::new(),
        })
    }

    fn from_ndb(bytes: &[u8]) -> EditorResult<Self> {
        let ndb = read_ndb(&mut Cursor::new(bytes)).map_err(display_error)?;
        Ok(Self {
            primary:     ScriptDebugPrimary::Ndb,
            primary_raw: bytes.to_vec(),
            ncs:         None,
            ndb:         Some(ndb),
            langspec:    None,
            sources:     BTreeMap::new(),
        })
    }

    fn configure(&mut self, request: &Value) -> EditorResult<()> {
        if let Some(encoded) = request.get("ncs").and_then(Value::as_str) {
            let bytes = BASE64
                .decode(encoded)
                .map_err(|error| format!("invalid NCS companion payload: {error}"))?;
            decode_ncs_header(&bytes).map_err(display_error)?;
            decode_ncs_instructions(&bytes).map_err(display_error)?;
            self.ncs = Some(bytes);
        }
        if let Some(encoded) = request.get("ndb").and_then(Value::as_str) {
            let bytes = BASE64
                .decode(encoded)
                .map_err(|error| format!("invalid NDB companion payload: {error}"))?;
            self.ndb = Some(read_ndb(&mut Cursor::new(bytes)).map_err(display_error)?);
        }
        if let Some(encoded) = request.get("langspec").and_then(Value::as_str) {
            let bytes = BASE64
                .decode(encoded)
                .map_err(|error| format!("invalid nwscript.nss payload: {error}"))?;
            self.langspec =
                Some(parse_langspec_bytes("nwscript.nss", &bytes).map_err(display_error)?);
        }
        if let Some(source_values) = request.get("sources").and_then(Value::as_object) {
            self.sources.clear();
            for (name, encoded) in source_values {
                let encoded = encoded
                    .as_str()
                    .ok_or_else(|| format!("source payload for {name} must be base64 text"))?;
                let bytes = BASE64
                    .decode(encoded)
                    .map_err(|error| format!("invalid source payload for {name}: {error}"))?;
                self.sources.insert(
                    name.clone(),
                    String::from_utf8_lossy(&bytes)
                        .lines()
                        .map(str::to_string)
                        .collect(),
                );
            }
        }
        Ok(())
    }

    fn snapshot(&self) -> EditorResult<Value> {
        let source_files = self
            .ndb
            .as_ref()
            .map(|ndb| {
                ndb.files
                    .iter()
                    .enumerate()
                    .map(|(index, file)| {
                        json!({
                            "index": index,
                            "name": file.name,
                            "isRoot": file.is_root,
                            "available": self.sources.contains_key(&file.name),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let Some(bytes) = self.ncs.as_deref() else {
            return Ok(json!({
                "primary": "ndb",
                "hasNcs": false,
                "hasNdb": true,
                "hasLangspec": self.langspec.is_some(),
                "sourceFiles": source_files,
                "summary": self.debug_summary(),
                "functions": self.debug_functions(&[]),
                "instructions": [],
                "diagnostics": ["Matching NCS bytecode was not resolved."],
            }));
        };

        let header = decode_ncs_header(bytes).map_err(display_error)?;
        let decoded = decode_ncs_instructions(bytes).map_err(display_error)?;
        let asm = disassemble_ncs(
            bytes,
            self.langspec.as_ref(),
            NcsDisassemblyOptions {
                max_string_length: 4096,
                ..NcsDisassemblyOptions::default()
            },
        )
        .map_err(display_error)?;
        if decoded.len() != asm.len() {
            return Err("NCS decoder and disassembler produced different instruction counts".into());
        }
        let code_size = decoded
            .iter()
            .map(NcsInstruction::encoded_len)
            .sum::<usize>();
        let functions = self.workbench_functions(&decoded, code_size);
        let valid_offsets = instruction_offsets(&decoded)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut diagnostics = Vec::new();
        if usize::try_from(header.code_size).unwrap_or(usize::MAX) != bytes.len() {
            diagnostics.push(format!(
                "Header declares {} bytes but the resource contains {} bytes.",
                header.code_size,
                bytes.len()
            ));
        }
        let offsets = instruction_offsets(&decoded);
        let instructions = decoded
            .iter()
            .zip(&asm)
            .zip(&offsets)
            .enumerate()
            .map(|(index, ((instruction, rendered), offset))| {
                let target = ncs_jump_target(instruction, *offset);
                if let Some(target) = target
                    && !valid_offsets.contains(&target)
                {
                    diagnostics.push(format!(
                        "Instruction at offset {offset} targets non-instruction offset {target}."
                    ));
                }
                let function_index = functions
                    .iter()
                    .position(|function| *offset >= function.start && *offset < function.end);
                let next_offset = offsets.get(index + 1).copied();
                let successors = instruction_successors(instruction.opcode, target, next_offset);
                let source = self.source_location(*offset);
                let action = self.action_details(instruction);
                let raw = std::iter::once(instruction.opcode as u8)
                    .chain(std::iter::once(instruction.auxcode as u8))
                    .chain(instruction.extra.iter().copied())
                    .collect::<Vec<_>>();
                json!({
                    "index": index,
                    "offset": offset,
                    "localOffset": function_index.map(|function_index| offset.saturating_sub(functions[function_index].start)),
                    "size": instruction.encoded_len(),
                    "label": rendered.label,
                    "opcode": instruction.opcode.canonical_name(),
                    "opcodeInternal": instruction.opcode.internal_name(),
                    "auxcode": instruction.auxcode.canonical_name(),
                    "auxcodeInternal": instruction.auxcode.internal_name(),
                    "operand": rendered.extra,
                    "action": action,
                    "rawHex": hex_bytes(&raw),
                    "jumpTarget": target,
                    "callTarget": (instruction.opcode == NcsOpcode::Jsr).then_some(target).flatten(),
                    "successors": successors,
                    "functionIndex": function_index,
                    "source": source,
                })
            })
            .collect::<Vec<_>>();
        let function_values = functions
            .iter()
            .enumerate()
            .map(|(index, function)| {
                let blocks = control_flow_blocks(index, function, &decoded, &offsets);
                json!({
                    "index": index,
                    "name": function.name,
                    "start": function.start,
                    "end": function.end,
                    "returnType": function.return_type,
                    "arguments": function.arguments,
                    "synthetic": function.synthetic,
                    "source": function.source,
                    "blocks": blocks,
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "primary": if self.primary == ScriptDebugPrimary::Ncs { "ncs" } else { "ndb" },
            "hasNcs": true,
            "hasNdb": self.ndb.is_some(),
            "hasLangspec": self.langspec.is_some(),
            "sourceFiles": source_files,
            "header": {
                "format": "NCS V1.0",
                "fileSize": bytes.len(),
                "declaredSize": header.code_size,
                "codeSize": code_size,
                "instructionCount": decoded.len(),
            },
            "summary": self.debug_summary(),
            "functions": function_values,
            "instructions": instructions,
            "diagnostics": diagnostics,
        }))
    }

    fn debug_summary(&self) -> Value {
        let Some(ndb) = &self.ndb else {
            return json!({ "files": 0, "structs": 0, "functions": 0, "variables": 0, "lineMappings": 0 });
        };
        json!({
            "files": ndb.files.len(),
            "structs": ndb.structs.len(),
            "functions": ndb.functions.len(),
            "variables": ndb.variables.len(),
            "lineMappings": ndb.lines.len(),
            "structEntries": ndb.structs.iter().map(|entry| json!({
                "name": entry.label,
                "fields": entry.fields.iter().map(|field| json!({ "name": field.label, "type": field.ty.to_string() })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "variableEntries": ndb.variables.iter().map(|entry| json!({
                "name": entry.label,
                "type": entry.ty.to_string(),
                "start": entry.binary_start.saturating_sub(NCS_BINARY_HEADER_SIZE as u32),
                "end": entry.binary_end.saturating_sub(NCS_BINARY_HEADER_SIZE as u32),
                "stackLocation": entry.stack_loc,
            })).collect::<Vec<_>>(),
        })
    }

    fn debug_functions(&self, _decoded: &[NcsInstruction]) -> Vec<Value> {
        self.ndb
            .as_ref()
            .map(|ndb| {
                ndb.functions
                    .iter()
                    .enumerate()
                    .map(|(index, function)| json!({
                        "index": index,
                        "name": function.label,
                        "start": function.binary_start.saturating_sub(NCS_BINARY_HEADER_SIZE as u32),
                        "end": function.binary_end.saturating_sub(NCS_BINARY_HEADER_SIZE as u32),
                        "returnType": function.return_type.to_string(),
                        "arguments": function.args.iter().map(ToString::to_string).collect::<Vec<_>>(),
                        "synthetic": false,
                        "blocks": [],
                    }))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn workbench_functions(
        &self,
        decoded: &[NcsInstruction],
        code_size: usize,
    ) -> Vec<WorkbenchFunction> {
        if let Some(ndb) = &self.ndb
            && !ndb.functions.is_empty()
        {
            let mut debug_functions = ndb
                .functions
                .iter()
                .map(|function| {
                    let start = function
                        .binary_start
                        .saturating_sub(NCS_BINARY_HEADER_SIZE as u32)
                        as usize;
                    let end = function
                        .binary_end
                        .saturating_sub(NCS_BINARY_HEADER_SIZE as u32)
                        as usize;
                    WorkbenchFunction {
                        name: function.label.clone(),
                        start,
                        end: end.max(start).min(code_size),
                        return_type: function.return_type.to_string(),
                        arguments: function.args.iter().map(ToString::to_string).collect(),
                        synthetic: false,
                        source: self.source_location(start),
                    }
                })
                .collect::<Vec<_>>();
            debug_functions.sort_by_key(|function| function.start);
            let mut functions = Vec::new();
            let mut covered_until = 0usize;
            for function in debug_functions {
                if function.start > covered_until {
                    functions.push(WorkbenchFunction {
                        name:        if covered_until == 0 {
                            "entry".to_string()
                        } else {
                            format!("code_{covered_until:04}")
                        },
                        start:       covered_until,
                        end:         function.start.min(code_size),
                        return_type: "?".to_string(),
                        arguments:   Vec::new(),
                        synthetic:   true,
                        source:      self.source_location(covered_until),
                    });
                }
                covered_until = covered_until.max(function.end);
                functions.push(function);
            }
            if covered_until < code_size {
                functions.push(WorkbenchFunction {
                    name:        format!("code_{covered_until:04}"),
                    start:       covered_until,
                    end:         code_size,
                    return_type: "?".to_string(),
                    arguments:   Vec::new(),
                    synthetic:   true,
                    source:      self.source_location(covered_until),
                });
            }
            return functions;
        }
        let offsets = instruction_offsets(decoded);
        let mut entries = BTreeSet::from([0usize]);
        for (instruction, offset) in decoded.iter().zip(&offsets) {
            if instruction.opcode == NcsOpcode::Jsr
                && let Some(target) = ncs_jump_target(instruction, *offset)
            {
                entries.insert(target);
            }
        }
        let entries = entries
            .into_iter()
            .filter(|offset| *offset < code_size)
            .collect::<Vec<_>>();
        entries
            .iter()
            .enumerate()
            .map(|(index, start)| WorkbenchFunction {
                name:        if index == 0 {
                    "entry".to_string()
                } else {
                    format!("sub_{start:04}")
                },
                start:       *start,
                end:         entries.get(index + 1).copied().unwrap_or(code_size),
                return_type: "?".to_string(),
                arguments:   Vec::new(),
                synthetic:   true,
                source:      None,
            })
            .collect()
    }

    fn source_location(&self, offset: usize) -> Option<Value> {
        let ndb = self.ndb.as_ref()?;
        let matches = ndb
            .lines
            .iter()
            .filter(|line| {
                let start =
                    line.binary_start
                        .saturating_sub(NCS_BINARY_HEADER_SIZE as u32) as usize;
                let end =
                    line.binary_end
                        .saturating_sub(NCS_BINARY_HEADER_SIZE as u32) as usize;
                (start..end).contains(&offset)
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        let line = matches[0];
        let file = ndb.files.get(line.file_num)?;
        let text = self
            .sources
            .get(&file.name)
            .and_then(|lines| lines.get(line.line_num.saturating_sub(1)))
            .map(|line| line.trim().to_string());
        Some(json!({
            "file": file.name,
            "line": line.line_num,
            "text": text,
            "available": self.sources.contains_key(&file.name),
        }))
    }

    fn action_details(&self, instruction: &NcsInstruction) -> Option<Value> {
        if instruction.opcode != NcsOpcode::ExecuteCommand || instruction.extra.len() != 3 {
            return None;
        }
        let id = usize::from(u16::from_be_bytes(
            instruction.extra.get(..2)?.try_into().ok()?,
        ));
        let argument_count = instruction.extra[2];
        let function = self.langspec.as_ref()?.functions.get(id)?;
        Some(json!({
            "id": id,
            "argumentCount": argument_count,
            "name": function.name,
            "returnType": function.return_type,
            "parameters": function.parameters,
            "arityMatches": usize::from(argument_count) == function.parameters.len(),
        }))
    }
}

struct WorkbenchFunction {
    name:        String,
    start:       usize,
    end:         usize,
    return_type: String,
    arguments:   Vec<String>,
    synthetic:   bool,
    source:      Option<Value>,
}

fn instruction_offsets(instructions: &[NcsInstruction]) -> Vec<usize> {
    instructions
        .iter()
        .scan(0usize, |offset, instruction| {
            let current = *offset;
            *offset = offset.saturating_add(instruction.encoded_len());
            Some(current)
        })
        .collect()
}

fn ncs_jump_target(instruction: &NcsInstruction, offset: usize) -> Option<usize> {
    if !matches!(
        instruction.opcode,
        NcsOpcode::Jmp | NcsOpcode::Jsr | NcsOpcode::Jz | NcsOpcode::Jnz
    ) || instruction.extra.len() != 4
    {
        return None;
    }
    let relative = i32::from_be_bytes(instruction.extra.as_slice().try_into().ok()?);
    let origin = i64::try_from(offset).ok()?;
    usize::try_from(origin.saturating_add(i64::from(relative))).ok()
}

fn instruction_successors(
    opcode: NcsOpcode,
    target: Option<usize>,
    next: Option<usize>,
) -> Vec<Value> {
    let mut successors = Vec::new();
    match opcode {
        NcsOpcode::Jmp => {
            if let Some(target) = target {
                successors.push(json!({ "offset": target, "kind": "jump" }));
            }
        }
        NcsOpcode::Jz | NcsOpcode::Jnz => {
            if let Some(target) = target {
                successors.push(json!({ "offset": target, "kind": "branch" }));
            }
            if let Some(next) = next {
                successors.push(json!({ "offset": next, "kind": "fallthrough" }));
            }
        }
        NcsOpcode::Ret => {}
        _ => {
            if let Some(next) = next {
                successors.push(json!({ "offset": next, "kind": "fallthrough" }));
            }
        }
    }
    successors
}

fn control_flow_blocks(
    function_index: usize,
    function: &WorkbenchFunction,
    instructions: &[NcsInstruction],
    offsets: &[usize],
) -> Vec<Value> {
    let function_rows = offsets
        .iter()
        .enumerate()
        .filter(|(_index, offset)| **offset >= function.start && **offset < function.end)
        .collect::<Vec<_>>();
    if function_rows.is_empty() {
        return Vec::new();
    }
    let valid = function_rows
        .iter()
        .map(|(_index, offset)| **offset)
        .collect::<BTreeSet<_>>();
    let mut leaders = BTreeSet::from([function.start]);
    for (row, (instruction_index, offset)) in function_rows.iter().enumerate() {
        let instruction = &instructions[*instruction_index];
        if let Some(target) = ncs_jump_target(instruction, **offset)
            && valid.contains(&target)
        {
            leaders.insert(target);
        }
        if matches!(
            instruction.opcode,
            NcsOpcode::Jmp | NcsOpcode::Jz | NcsOpcode::Jnz | NcsOpcode::Ret
        ) && let Some((_next_index, next_offset)) = function_rows.get(row + 1)
        {
            leaders.insert(**next_offset);
        }
    }
    let leaders = leaders.into_iter().collect::<Vec<_>>();
    leaders
        .iter()
        .enumerate()
        .map(|(block_index, start)| {
            let end = leaders
                .get(block_index + 1)
                .copied()
                .unwrap_or(function.end);
            let rows = function_rows
                .iter()
                .filter(|(_index, offset)| **offset >= *start && **offset < end)
                .map(|(index, _offset)| *index)
                .collect::<Vec<_>>();
            let last = rows.last().copied();
            let successors = last.map_or_else(Vec::new, |last| {
                let target = ncs_jump_target(&instructions[last], offsets[last]);
                let next = offsets
                    .get(last + 1)
                    .copied()
                    .filter(|offset| *offset < function.end);
                instruction_successors(instructions[last].opcode, target, next)
                    .into_iter()
                    .filter(|successor| {
                        successor
                            .get("offset")
                            .and_then(Value::as_u64)
                            .is_some_and(|offset| valid.contains(&(offset as usize)))
                    })
                    .collect()
            });
            let calls = rows
                .iter()
                .filter_map(|index| {
                    (instructions[*index].opcode == NcsOpcode::Jsr)
                        .then(|| ncs_jump_target(&instructions[*index], offsets[*index]))
                        .flatten()
                })
                .collect::<Vec<_>>();
            json!({
                "id": format!("f{function_index}b{block_index}"),
                "start": start,
                "end": end,
                "instructionIndices": rows,
                "successors": successors,
                "calls": calls,
            })
        })
        .collect()
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_gff_extension(extension: &str) -> bool {
    matches!(
        extension,
        "gff"
            | "utc"
            | "utd"
            | "ute"
            | "uti"
            | "utm"
            | "utp"
            | "uts"
            | "utt"
            | "utw"
            | "git"
            | "are"
            | "gic"
            | "ifo"
            | "fac"
            | "dlg"
            | "itp"
            | "bic"
            | "jrl"
            | "gui"
    )
}

fn gff_source_encoding(path: &Path) -> Option<GffSourceEncoding> {
    let extension = path.extension()?.to_str()?;
    if is_gff_extension(&extension.to_ascii_lowercase()) {
        return Some(GffSourceEncoding::Binary);
    }
    if !extension.eq_ignore_ascii_case("json") {
        return None;
    }
    let resource_name = path.file_stem()?.to_str()?;
    let resource_extension = Path::new(resource_name).extension()?.to_str()?;
    is_gff_extension(&resource_extension.to_ascii_lowercase()).then_some(GffSourceEncoding::Json)
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn required_string<'a>(value: &'a Value, field: &str) -> EditorResult<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field} is required"))
}

fn required_path(value: &Value, field: &str) -> EditorResult<PathBuf> {
    required_string(value, field).map(PathBuf::from)
}

fn page_limit(request: &Value) -> usize {
    request
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> EditorResult<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("resource");
    let mut attempt = 0_u32;
    let (temporary_path, mut temporary) = loop {
        let candidate = parent.join(format!(
            ".{filename}.nwnrs-{}-{attempt}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(error) => {
                return Err(format!(
                    "failed to create temporary file {}: {error}",
                    candidate.display()
                ));
            }
        }
    };
    let result = (|| -> io::Result<()> {
        temporary.write_all(bytes)?;
        temporary.sync_all()?;
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&temporary_path, metadata.permissions())?;
        }
        drop(temporary);
        fs::rename(&temporary_path, path)?;
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "failed to atomically write {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

fn texture_snapshot(width: u32, height: u32, rgba: &[u8], metadata: Value) -> EditorResult<Value> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "texture dimensions overflow".to_string())?;
    if rgba.len() != expected {
        return Err(format!(
            "texture expected {expected} RGBA bytes, got {}",
            rgba.len()
        ));
    }
    Ok(json!({
        "width": width,
        "height": height,
        "rgba": BASE64.encode(rgba),
        "metadata": metadata,
    }))
}

fn decode_texture_edit(edit: &Value) -> EditorResult<(u32, u32, Vec<u8>)> {
    let width = edit
        .get("width")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "texture width must be a positive 32-bit integer".to_string())?;
    let height = edit
        .get("height")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "texture height must be a positive 32-bit integer".to_string())?;
    let rgba = BASE64
        .decode(required_string(edit, "rgba")?)
        .map_err(|error| format!("invalid RGBA payload: {error}"))?;
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "texture dimensions overflow".to_string())?;
    if rgba.len() != expected {
        return Err(format!(
            "texture expected {expected} RGBA bytes, got {}",
            rgba.len()
        ));
    }
    Ok((width, height, rgba))
}

fn encoded_texture_inverse(bytes: Vec<u8>) -> Value {
    json!({ "action": "restoreTextureBytes", "contents": BASE64.encode(bytes) })
}

fn decode_encoded_texture(edit: &Value) -> EditorResult<Vec<u8>> {
    BASE64
        .decode(required_string(edit, "contents")?)
        .map_err(|error| format!("invalid encoded texture payload: {error}"))
}

fn write_dds_bytes(value: &DdsTexture) -> EditorResult<Vec<u8>> {
    let mut bytes = Vec::new();
    write_dds(&mut bytes, value).map_err(display_error)?;
    Ok(bytes)
}

fn write_tga_bytes(value: &TgaTexture) -> EditorResult<Vec<u8>> {
    let mut bytes = Vec::new();
    write_tga(&mut bytes, value).map_err(display_error)?;
    Ok(bytes)
}

fn apply_plt_edit(value: &mut PltTexture, edit: &Value) -> EditorResult<(String, Value)> {
    let x = edit
        .get("x")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| "x is required".to_string())?;
    let y = edit
        .get("y")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| "y is required".to_string())?;
    let pixel = value.pixel_at(x, y).map_err(display_error)?;
    let new_value = edit
        .get("value")
        .and_then(Value::as_u64)
        .and_then(|v| u8::try_from(v).ok())
        .ok_or_else(|| "PLT value must be between 0 and 255".to_string())?;
    let layer = edit
        .get("layer")
        .and_then(Value::as_u64)
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| *v <= 9)
        .ok_or_else(|| "PLT layer must be between 0 and 9".to_string())?;
    let index = usize::try_from(y)
        .ok()
        .and_then(|row| {
            usize::try_from(value.width)
                .ok()
                .and_then(|width| row.checked_mul(width))
        })
        .and_then(|row| {
            usize::try_from(x)
                .ok()
                .and_then(|column| row.checked_add(column))
        })
        .ok_or_else(|| "PLT pixel index overflow".to_string())?;
    let slot = value
        .pixels
        .get_mut(index)
        .ok_or_else(|| "PLT pixel out of range".to_string())?;
    *slot = PltPixel {
        value:    new_value,
        layer_id: layer,
    };
    Ok((
        "Edit PLT pixel".to_string(),
        json!({ "action": "setPltPixel", "x": x, "y": y, "value": pixel.value, "layer": pixel.layer_id }),
    ))
}

fn twoda_to_json(value: &TwoDa) -> Value {
    let rows = value
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            json!({
                "label": value.row_label(index).unwrap_or(""),
                "cells": row,
            })
        })
        .collect::<Vec<_>>();
    json!({ "columns": value.columns(), "default": value.default(), "rows": rows })
}

fn apply_twoda_edit(
    value: &mut TwoDa,
    action: &str,
    edit: &Value,
) -> EditorResult<(String, Value)> {
    match action {
        "set2daCell" => {
            let row = json_usize(edit, "row")?;
            let column = required_string(edit, "column")?;
            let old = value
                .rows
                .get(row)
                .and_then(|row_data| {
                    value
                        .columns()
                        .iter()
                        .position(|candidate| candidate.eq_ignore_ascii_case(column))
                        .and_then(|index| row_data.get(index).cloned())
                })
                .flatten();
            let new_value = optional_cell(edit.get("value"))?;
            value
                .set_cell(row, column, new_value)
                .map_err(display_error)?;
            Ok((
                "Edit 2DA cell".to_string(),
                json!({ "action": action, "row": row, "column": column, "value": old }),
            ))
        }
        "set2daRowLabel" => {
            let row = json_usize(edit, "row")?;
            let before = value
                .row_label(row)
                .ok_or_else(|| "2DA row out of bounds".to_string())?
                .to_string();
            let label = required_string(edit, "label")?;
            value.set_row_label(row, label).map_err(display_error)?;
            Ok((
                "Edit 2DA row label".to_string(),
                json!({ "action": action, "row": row, "label": before }),
            ))
        }
        "replace2da" => {
            let before = twoda_to_json(value);
            let mut candidate = value.clone();
            replace_twoda(
                &mut candidate,
                edit.get("table")
                    .ok_or_else(|| "table is required".to_string())?,
            )?;
            *value = candidate;
            Ok((
                "Edit 2DA structure".to_string(),
                json!({ "action": action, "table": before }),
            ))
        }
        _ => Err(format!("unknown 2DA edit: {action}")),
    }
}

fn replace_twoda(value: &mut TwoDa, table: &Value) -> EditorResult<()> {
    let columns = table
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| "2DA columns are required".to_string())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "2DA column names must be strings".to_string())
        })
        .collect::<EditorResult<Vec<_>>>()?;
    let rows_json = table
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| "2DA rows are required".to_string())?;
    let mut rows = Vec::with_capacity(rows_json.len());
    let mut labels = Vec::with_capacity(rows_json.len());
    for row in rows_json {
        labels.push(required_string(row, "label")?.to_string());
        let cells = row
            .get("cells")
            .and_then(Value::as_array)
            .ok_or_else(|| "2DA row cells are required".to_string())?
            .iter()
            .map(|cell| optional_cell(Some(cell)))
            .collect::<EditorResult<Vec<_>>>()?;
        rows.push(cells);
    }
    value.set_columns(columns).map_err(display_error)?;
    value.replace_rows(rows, labels).map_err(display_error)?;
    value.set_default(optional_cell(table.get("default"))?);
    Ok(())
}

fn optional_cell(value: Option<&Value>) -> EditorResult<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err("2DA cells must be strings or null".to_string()),
    }
}

fn apply_tlk_edit(
    value: &mut SingleTlk,
    action: &str,
    edit: &Value,
) -> EditorResult<(String, Value)> {
    match action {
        "setTlkEntry" => {
            let str_ref = edit
                .get("strRef")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| "strRef must be a 32-bit unsigned integer".to_string())?;
            let had_override = value.has_override(str_ref);
            let before = value
                .get(str_ref)
                .map_err(display_error)?
                .map(|entry| tlk_entry_to_json(str_ref, &entry));
            match edit.get("entry") {
                None | Some(Value::Null) => {
                    value.remove_override(str_ref);
                }
                Some(entry) => value.set_entry(str_ref, tlk_entry_from_json(entry)?),
            }
            let inverse = if had_override {
                json!({ "action": action, "strRef": str_ref, "entry": before })
            } else {
                json!({ "action": "clearTlkOverride", "strRef": str_ref })
            };
            Ok(("Edit TLK entry".to_string(), inverse))
        }
        "clearTlkOverride" => {
            let str_ref = edit
                .get("strRef")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| "strRef must be a 32-bit unsigned integer".to_string())?;
            let before = value
                .get(str_ref)
                .map_err(display_error)?
                .map(|entry| tlk_entry_to_json(str_ref, &entry));
            value.remove_override(str_ref);
            Ok((
                "Restore TLK entry".to_string(),
                json!({ "action": "setTlkEntry", "strRef": str_ref, "entry": before }),
            ))
        }
        "setTlkLanguage" => {
            let before = value.language.id();
            let language = edit
                .get("language")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .and_then(Language::from_id)
                .ok_or_else(|| "unsupported TLK language id".to_string())?;
            value.language = language;
            Ok((
                "Change TLK language".to_string(),
                json!({ "action": action, "language": before }),
            ))
        }
        _ => Err(format!("unknown TLK edit: {action}")),
    }
}

fn tlk_snapshot(value: &mut SingleTlk, request: &Value) -> EditorResult<Value> {
    let offset = request
        .get("offset")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let limit = page_limit(request);
    let query = request
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let highest = value.highest();
    if query.is_empty() {
        let total = usize::try_from(highest.saturating_add(1)).unwrap_or(usize::MAX);
        let end = offset.saturating_add(limit).min(total);
        let mut entries = Vec::with_capacity(end.saturating_sub(offset));
        for index in offset..end {
            let str_ref = u32::try_from(index)
                .map_err(|error| format!("TLK string reference exceeds 32-bit range: {error}"))?;
            if let Some(entry) = value.get(str_ref).map_err(display_error)? {
                entries.push(tlk_entry_to_json(str_ref, &entry));
            }
        }
        return Ok(json!({
            "language": value.language.id(),
            "highest": highest,
            "total": total,
            "offset": offset,
            "limit": limit,
            "entries": entries,
        }));
    }
    let mut matched = 0_usize;
    let mut entries = Vec::with_capacity(limit);
    if highest >= 0 {
        for str_ref in 0..=u32::try_from(highest).unwrap_or(u32::MAX) {
            let Some(entry) = value.get(str_ref).map_err(display_error)? else {
                continue;
            };
            if !str_ref.to_string().contains(&query)
                && !entry.text.to_ascii_lowercase().contains(&query)
                && !entry.sound_res_ref.to_ascii_lowercase().contains(&query)
            {
                continue;
            }
            if matched >= offset && entries.len() < limit {
                entries.push(tlk_entry_to_json(str_ref, &entry));
            }
            matched = matched.saturating_add(1);
        }
    }
    Ok(json!({
        "language": value.language.id(),
        "highest": highest,
        "total": matched,
        "offset": offset,
        "limit": limit,
        "entries": entries,
    }))
}

fn tlk_entry_to_json(str_ref: u32, entry: &TlkEntry) -> Value {
    json!({
        "strRef": str_ref,
        "text": entry.text,
        "soundResRef": entry.sound_res_ref,
        "soundLength": entry.sound_length,
        "flags": entry.flags,
        "volumeVariance": entry.volume_variance,
        "pitchVariance": entry.pitch_variance,
    })
}

fn tlk_entry_from_json(value: &Value) -> EditorResult<TlkEntry> {
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let sound_res_ref = value
        .get("soundResRef")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if sound_res_ref.len() > 16 {
        return Err("TLK sound resource references cannot exceed 16 bytes".to_string());
    }
    let sound_length = checked_f32(
        value
            .get("soundLength")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        "TLK sound length",
    )?;
    let mut entry = TlkEntry::new(text, sound_res_ref, sound_length);
    entry.flags = value
        .get("flags")
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(entry.flags);
    entry.volume_variance = value
        .get("volumeVariance")
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(0);
    entry.pitch_variance = value
        .get("pitchVariance")
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(0);
    Ok(entry)
}

fn json_usize(value: &Value, field: &str) -> EditorResult<usize> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| format!("{field} must be a non-negative integer"))
}

fn gff_root_to_json(value: &GffRoot) -> Value {
    json!({
        "fileType": value.file_type,
        "fileVersion": value.file_version,
        "root": gff_struct_to_json(&value.root),
    })
}

fn gff_struct_to_json(value: &GffStruct) -> Value {
    json!({
        "id": value.id,
        "fields": value.fields().iter().map(|(label, field)| json!({
            "label": label,
            "kind": gff_kind_name(field.value()),
            "value": gff_value_to_json(field.value()),
        })).collect::<Vec<_>>(),
    })
}

fn gff_kind_name(value: &GffValue) -> &'static str {
    match value {
        GffValue::Byte(_) => "byte",
        GffValue::Char(_) => "char",
        GffValue::Word(_) => "word",
        GffValue::Short(_) => "short",
        GffValue::Dword(_) => "dword",
        GffValue::Int(_) => "int",
        GffValue::Float(_) => "float",
        GffValue::Dword64(_) => "dword64",
        GffValue::Int64(_) => "int64",
        GffValue::Double(_) => "double",
        GffValue::CExoString(_) => "string",
        GffValue::ResRef(_) => "resref",
        GffValue::CExoLocString(_) => "locstring",
        GffValue::Void(_) => "void",
        GffValue::Struct(_) => "struct",
        GffValue::List(_) => "list",
    }
}

fn gff_value_to_json(value: &GffValue) -> Value {
    match value {
        GffValue::Byte(v) => json!(v),
        GffValue::Char(v) => json!(v),
        GffValue::Word(v) => json!(v),
        GffValue::Short(v) => json!(v),
        GffValue::Dword(v) => json!(v),
        GffValue::Int(v) => json!(v),
        GffValue::Float(v) => json!(v),
        GffValue::Dword64(v) => json!(v.to_string()),
        GffValue::Int64(v) => json!(v.to_string()),
        GffValue::Double(v) => json!(v),
        GffValue::CExoString(v) | GffValue::ResRef(v) => json!(v),
        GffValue::CExoLocString(v) => {
            json!({ "strRef": v.str_ref, "entries": v.entries.iter().map(|(language, text)| json!({ "language": language, "text": text })).collect::<Vec<_>>() })
        }
        GffValue::Void(v) => json!(BASE64.encode(v)),
        GffValue::Struct(v) => gff_struct_to_json(v),
        GffValue::List(v) => Value::Array(v.iter().map(gff_struct_to_json).collect()),
    }
}

fn merge_gff_root(target: &mut GffRoot, value: &Value) -> EditorResult<()> {
    let file_type = required_string(value, "fileType")?;
    let file_version = required_string(value, "fileVersion")?;
    if file_type.len() != 4 || file_version.len() != 4 {
        return Err("GFF file type and version must be exactly four bytes".to_string());
    }
    target.file_type = file_type.to_string();
    target.file_version = file_version.to_string();
    merge_gff_struct(
        &mut target.root,
        value
            .get("root")
            .ok_or_else(|| "GFF root is required".to_string())?,
    )
}

fn merge_gff_struct(target: &mut GffStruct, value: &Value) -> EditorResult<()> {
    target.id = value
        .get("id")
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| "GFF struct id must be a 32-bit integer".to_string())?;
    let fields = value
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(|| "GFF fields are required".to_string())?;
    let labels = fields
        .iter()
        .map(|field| required_string(field, "label").map(str::to_string))
        .collect::<EditorResult<Vec<_>>>()?;
    let existing = target
        .fields()
        .iter()
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    for label in existing {
        if !labels.contains(&label) {
            target.remove(&label);
        }
    }
    for field_json in fields {
        let label = required_string(field_json, "label")?;
        let kind = required_string(field_json, "kind")?;
        let raw_value = field_json
            .get("value")
            .ok_or_else(|| format!("GFF field {label} has no value"))?;
        if let Some(field) = target.get_field_mut(label)
            && gff_kind_name(field.value()) == kind
        {
            merge_gff_value(field.value_mut(), kind, raw_value)?;
            continue;
        }
        target
            .put_field(label, GffField::new(gff_value_from_json(kind, raw_value)?))
            .map_err(display_error)?;
    }
    Ok(())
}

fn merge_gff_value(target: &mut GffValue, kind: &str, value: &Value) -> EditorResult<()> {
    match (target, kind) {
        (GffValue::Struct(target), "struct") => merge_gff_struct(target, value),
        (GffValue::List(target), "list") => {
            let values = value
                .as_array()
                .ok_or_else(|| "GFF list value must be an array".to_string())?;
            for (index, source) in values.iter().enumerate() {
                if let Some(existing) = target.get_mut(index) {
                    merge_gff_struct(existing, source)?;
                } else {
                    target.push(gff_struct_from_json(source)?);
                }
            }
            target.truncate(values.len());
            Ok(())
        }
        (target, _) => {
            *target = gff_value_from_json(kind, value)?;
            Ok(())
        }
    }
}

fn gff_struct_from_json(value: &Value) -> EditorResult<GffStruct> {
    let id = value
        .get("id")
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| "GFF struct id must be a 32-bit integer".to_string())?;
    let mut result = GffStruct::new(id);
    merge_gff_struct(&mut result, value)?;
    Ok(result)
}

fn gff_value_from_json(kind: &str, value: &Value) -> EditorResult<GffValue> {
    let signed = || {
        value
            .as_i64()
            .ok_or_else(|| format!("GFF {kind} must be an integer"))
    };
    let unsigned = || {
        value
            .as_u64()
            .ok_or_else(|| format!("GFF {kind} must be a non-negative integer"))
    };
    Ok(match kind {
        "byte" => GffValue::Byte(
            u8::try_from(unsigned()?).map_err(|error| format!("GFF byte out of range: {error}"))?,
        ),
        "char" => GffValue::Char(
            i8::try_from(signed()?).map_err(|error| format!("GFF char out of range: {error}"))?,
        ),
        "word" => GffValue::Word(
            u16::try_from(unsigned()?)
                .map_err(|error| format!("GFF word out of range: {error}"))?,
        ),
        "short" => GffValue::Short(
            i16::try_from(signed()?).map_err(|error| format!("GFF short out of range: {error}"))?,
        ),
        "dword" => GffValue::Dword(
            u32::try_from(unsigned()?)
                .map_err(|error| format!("GFF dword out of range: {error}"))?,
        ),
        "int" => GffValue::Int(
            i32::try_from(signed()?).map_err(|error| format!("GFF int out of range: {error}"))?,
        ),
        "float" => GffValue::Float(checked_f32(
            value
                .as_f64()
                .ok_or_else(|| "GFF float must be numeric".to_string())?,
            "GFF float",
        )?),
        "dword64" => GffValue::Dword64(
            required_json_integer_string(value)?
                .parse()
                .map_err(|error| format!("GFF dword64 out of range: {error}"))?,
        ),
        "int64" => GffValue::Int64(
            required_json_integer_string(value)?
                .parse()
                .map_err(|error| format!("GFF int64 out of range: {error}"))?,
        ),
        "double" => GffValue::Double(
            value
                .as_f64()
                .ok_or_else(|| "GFF double must be numeric".to_string())?,
        ),
        "string" => GffValue::CExoString(
            value
                .as_str()
                .ok_or_else(|| "GFF string must be text".to_string())?
                .to_string(),
        ),
        "resref" => {
            let text = value
                .as_str()
                .ok_or_else(|| "GFF resref must be text".to_string())?;
            if text.len() > 255 {
                return Err("GFF resref exceeds 255 bytes".to_string());
            }
            GffValue::ResRef(text.to_string())
        }
        "locstring" => {
            let str_ref = value
                .get("strRef")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| {
                    "GFF locstring strRef must be a 32-bit unsigned integer".to_string()
                })?;
            let entries = value
                .get("entries")
                .and_then(Value::as_array)
                .ok_or_else(|| "GFF locstring entries are required".to_string())?
                .iter()
                .map(|entry| {
                    Ok((
                        entry
                            .get("language")
                            .and_then(Value::as_i64)
                            .and_then(|v| i32::try_from(v).ok())
                            .ok_or_else(|| "invalid locstring language".to_string())?,
                        required_string(entry, "text")?.to_string(),
                    ))
                })
                .collect::<EditorResult<Vec<_>>>()?;
            GffValue::CExoLocString(GffCExoLocString {
                str_ref,
                entries,
            })
        }
        "void" => GffValue::Void(
            BASE64
                .decode(
                    value
                        .as_str()
                        .ok_or_else(|| "GFF void must be base64 text".to_string())?,
                )
                .map_err(|error| format!("invalid GFF void data: {error}"))?,
        ),
        "struct" => GffValue::Struct(gff_struct_from_json(value)?),
        "list" => GffValue::List(
            value
                .as_array()
                .ok_or_else(|| "GFF list must be an array".to_string())?
                .iter()
                .map(gff_struct_from_json)
                .collect::<EditorResult<Vec<_>>>()?,
        ),
        _ => return Err(format!("unsupported GFF field kind: {kind}")),
    })
}

fn required_json_integer_string(value: &Value) -> EditorResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err("64-bit GFF integers must be decimal strings".to_string()),
    }
}

fn checked_f32(value: f64, label: &str) -> EditorResult<f32> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(format!("{label} is outside the finite 32-bit float range"));
    }
    #[allow(clippy::cast_possible_truncation)]
    let converted = value as f32;
    Ok(converted)
}

struct ArchiveEntry {
    resref:      ResRef,
    original:    Option<Res>,
    replacement: Option<Vec<u8>>,
    algorithm:   Algorithm,
}

impl ArchiveEntry {
    fn bytes(&self) -> EditorResult<Vec<u8>> {
        if let Some(bytes) = &self.replacement {
            return Ok(bytes.clone());
        }
        self.original
            .as_ref()
            .ok_or_else(|| format!("missing payload for {}", self.resref))?
            .read_all(CachePolicy::Bypass)
            .map_err(display_error)
    }
}

struct ErfDocument {
    metadata: Erf,
    entries:  Vec<ArchiveEntry>,
}

impl ErfDocument {
    fn new(metadata: Erf) -> Result<Self, nwnrs_types::erf::ErfError> {
        let entries = metadata
            .entries()
            .iter()
            .map(|(resref, res)| ArchiveEntry {
                resref:      resref.clone(),
                original:    Some(res.clone()),
                replacement: None,
                algorithm:   res.compressed_buf_algorithm().unwrap_or(Algorithm::None),
            })
            .collect();
        Ok(Self {
            metadata,
            entries,
        })
    }

    fn snapshot(&self, request: &Value) -> Value {
        let (entries, total, offset, limit) = archive_page(&self.entries, request);
        json!({
            "fileType": self.metadata.file_type.trim(),
            "version": match self.metadata.file_version { ErfVersion::V1 => "V1.0", ErfVersion::E1 => "E1.0" },
            "buildYear": self.metadata.build_year,
            "buildDay": self.metadata.build_day,
            "strRef": self.metadata.str_ref,
            "oid": self.metadata.oid(),
            "localizedStrings": self.metadata.loc_strings().iter().map(|(language, text)| json!({ "language": language, "text": text })).collect::<Vec<_>>(),
            "entries": entries,
            "total": total,
            "offset": offset,
            "limit": limit,
            "query": request.get("query").and_then(Value::as_str).unwrap_or_default(),
        })
    }

    fn read_entry(&self, resource: &str) -> EditorResult<Vec<u8>> {
        self.entries
            .iter()
            .find(|entry| resource_name(&entry.resref) == resource)
            .ok_or_else(|| format!("archive resource not found: {resource}"))?
            .bytes()
    }

    fn apply_edit(&mut self, action: &str, edit: &Value) -> EditorResult<(String, Value)> {
        apply_archive_edit(&mut self.entries, action, edit)
    }

    fn write(&self, cursor: &mut Cursor<Vec<u8>>) -> EditorResult<()> {
        let entries = self
            .entries
            .iter()
            .map(|entry| entry.resref.clone())
            .collect::<Vec<_>>();
        let mut payloads = BTreeMap::new();
        let mut algorithms = BTreeMap::new();
        let mut exocomp = ExoResFileCompressionType::None;
        for entry in &self.entries {
            payloads.insert(entry.resref.clone(), entry.bytes()?);
            algorithms.insert(entry.resref.clone(), entry.algorithm);
            if entry.algorithm != Algorithm::None {
                exocomp = ExoResFileCompressionType::CompressedBuf;
            }
        }
        write_erf_with_options(
            cursor,
            &self.metadata.file_type,
            self.metadata.file_version,
            u32::try_from(self.metadata.build_year)
                .map_err(|error| format!("ERF build year cannot be negative: {error}"))?,
            u32::try_from(self.metadata.build_day)
                .map_err(|error| format!("ERF build day cannot be negative: {error}"))?,
            exocomp,
            Algorithm::None,
            self.metadata.loc_strings(),
            self.metadata.str_ref,
            &entries,
            self.metadata.oid(),
            ErfWriteOptions {
                resource_list_padding: self.metadata.resource_list_padding(),
            },
            |resref, output| {
                let bytes = payloads
                    .get(resref)
                    .ok_or_else(|| io::Error::other(format!("missing payload for {resref}")))?;
                output.write_all(bytes)?;
                Ok((bytes.len(), sha1_digest(bytes)))
            },
            |resref| algorithms.get(resref).copied().unwrap_or(Algorithm::None),
        )
        .map_err(display_error)
    }
}

struct KeyDocument {
    metadata: KeyTable,
    bifs:     Vec<KeyBifEntry>,
    entries:  Vec<ArchiveEntry>,
}

impl KeyDocument {
    fn new(metadata: KeyTable) -> Result<Self, nwnrs_types::key::KeyError> {
        let bifs = metadata.archive_layout()?;
        let entries = bifs
            .iter()
            .flat_map(|bif| bif.entries.iter())
            .map(|resref| {
                let res = metadata.demand(resref)?;
                Ok(ArchiveEntry {
                    resref:      resref.clone(),
                    original:    Some(res.clone()),
                    replacement: None,
                    algorithm:   res.compressed_buf_algorithm().unwrap_or(Algorithm::None),
                })
            })
            .collect::<Result<Vec<_>, nwnrs_types::resman::ResManError>>()
            .map_err(nwnrs_types::key::KeyError::from)?;
        Ok(Self {
            metadata,
            bifs,
            entries,
        })
    }

    fn snapshot(&self, request: &Value) -> Value {
        let bifs = self
            .bifs
            .iter()
            .enumerate()
            .map(|(index, bif)| {
                json!({
                    "index": index,
                    "filename": bif.recorded_filename.as_deref().unwrap_or(""),
                    "drives": bif.drives,
                    "oid": bif.bif_oid,
                    "entryCount": bif.entries.len(),
                })
            })
            .collect::<Vec<_>>();
        let query = request
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let offset = request
            .get("offset")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0);
        let limit = page_limit(request);
        let matches = |entry: &&ArchiveEntry| {
            query.is_empty()
                || resource_name(&entry.resref)
                    .to_ascii_lowercase()
                    .contains(&query)
        };
        let total = self.entries.iter().filter(matches).count();
        let entries = self
            .entries
            .iter()
            .filter(matches)
            .skip(offset)
            .take(limit)
            .map(|entry| {
                let mut value = archive_entry_json(entry);
                if let Some(object) = value.as_object_mut() {
                    let bif_index = self
                        .bifs
                        .iter()
                        .position(|bif| bif.entries.contains(&entry.resref));
                    object.insert("bifIndex".to_string(), json!(bif_index));
                    object.insert(
                        "bif".to_string(),
                        json!(
                            bif_index
                                .and_then(|index| self.bifs.get(index))
                                .and_then(|bif| bif.recorded_filename.as_deref())
                        ),
                    );
                }
                value
            })
            .collect::<Vec<_>>();
        json!({
            "version": match self.metadata.version() { KeyBifVersion::V1 => "V1.0", KeyBifVersion::E1 => "E1.0" },
            "buildYear": self.metadata.build_year(), "buildDay": self.metadata.build_day(),
            "oid": self.metadata.oid(), "bifs": bifs, "entries": entries,
            "total": total, "offset": offset, "limit": limit,
            "query": query,
        })
    }

    fn materialize(&mut self) -> EditorResult<()> {
        for entry in &mut self.entries {
            let bytes = entry.bytes()?;
            entry.original = None;
            entry.replacement = Some(bytes);
        }
        Ok(())
    }

    fn read_entry(&self, resource: &str) -> EditorResult<Vec<u8>> {
        self.entries
            .iter()
            .find(|entry| resource_name(&entry.resref) == resource)
            .ok_or_else(|| format!("KEY resource not found: {resource}"))?
            .bytes()
    }

    fn apply_edit(&mut self, action: &str, edit: &Value) -> EditorResult<(String, Value)> {
        if self.bifs.is_empty() {
            return Err("KEY table contains no BIF files".to_string());
        }
        let requested_bif = edit
            .get("bifIndex")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        if requested_bif.is_some_and(|index| index >= self.bifs.len()) {
            return Err("BIF index out of range".to_string());
        }
        let resource = edit.get("resource").and_then(Value::as_str);
        let owning_bif = resource.and_then(|resource| {
            self.entries
                .iter()
                .find(|entry| resource_name(&entry.resref) == resource)
                .and_then(|entry| {
                    self.bifs
                        .iter()
                        .position(|bif| bif.entries.contains(&entry.resref))
                })
        });
        let result = apply_archive_edit(&mut self.entries, action, edit)?;
        let target_bif = requested_bif.or(owning_bif);
        self.rebuild_bif_entries(target_bif)?;
        Ok(result)
    }

    fn rebuild_bif_entries(&mut self, requested_bif: Option<usize>) -> EditorResult<()> {
        if self.bifs.is_empty() {
            return Err("KEY table contains no BIF files".to_string());
        }
        let target = requested_bif.unwrap_or(0);
        if target >= self.bifs.len() {
            return Err("BIF index out of range".to_string());
        }
        let existing = self
            .bifs
            .iter()
            .flat_map(|bif| bif.entries.iter().cloned())
            .collect::<Vec<_>>();
        let live = self
            .entries
            .iter()
            .map(|entry| entry.resref.clone())
            .collect::<Vec<_>>();
        for bif in &mut self.bifs {
            bif.entries.retain(|entry| live.contains(entry));
        }
        let target_bif = self
            .bifs
            .get_mut(target)
            .ok_or_else(|| "BIF index out of range".to_string())?;
        for entry in &live {
            if !existing.contains(entry) {
                target_bif.entries.push(entry.clone());
            }
        }
        let mut by_resref = self
            .entries
            .drain(..)
            .map(|entry| (entry.resref.clone(), entry))
            .collect::<HashMap<_, _>>();
        for resref in self.bifs.iter().flat_map(|bif| &bif.entries) {
            if let Some(entry) = by_resref.remove(resref) {
                self.entries.push(entry);
            }
        }
        self.entries.extend(by_resref.into_values());
        Ok(())
    }

    fn write_atomic(&self, key_path: &Path) -> EditorResult<()> {
        let parent = key_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let key_name = key_path
            .file_stem()
            .and_then(|v| v.to_str())
            .ok_or_else(|| "KEY destination has no valid filename".to_string())?;
        let staging = parent.join(format!(
            ".nwnrs-key-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&staging).map_err(display_error)?;
        let result = (|| -> EditorResult<()> {
            let mut payloads = BTreeMap::new();
            for entry in &self.entries {
                payloads.insert(entry.resref.clone(), entry.bytes()?);
            }
            let exocomp = if self
                .entries
                .iter()
                .any(|entry| entry.algorithm != Algorithm::None)
            {
                ExoResFileCompressionType::CompressedBuf
            } else {
                ExoResFileCompressionType::None
            };
            let algorithm = self
                .entries
                .iter()
                .map(|entry| entry.algorithm)
                .find(|algorithm| *algorithm != Algorithm::None)
                .unwrap_or(Algorithm::None);
            write_key_and_bif(
                self.metadata.version(),
                exocomp,
                algorithm,
                &staging,
                key_name,
                "",
                &self.bifs,
                self.metadata.build_year(),
                self.metadata.build_day(),
                self.metadata.raw_oid(),
                |resref, output| {
                    let bytes = payloads
                        .get(resref)
                        .ok_or_else(|| io::Error::other(format!("missing payload for {resref}")))?;
                    output.write_all(bytes)?;
                    Ok((bytes.len(), sha1_digest(bytes)))
                },
            )
            .map_err(display_error)?;
            commit_key_staging(&staging, parent)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    fn backup_bytes(&self) -> EditorResult<Vec<u8>> {
        let temporary = std::env::temp_dir().join(format!(
            "nwnrs-key-backup-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&temporary).map_err(display_error)?;
        let key_path = temporary.join("backup.key");
        let result = self.write_atomic(&key_path).and_then(|_| {
            let mut files = Vec::new();
            collect_relative_files(&temporary, &temporary, &mut files)?;
            let bundle = files.into_iter().map(|relative| {
                let bytes = fs::read(temporary.join(&relative)).map_err(display_error)?;
                Ok(json!({ "path": relative.to_string_lossy(), "contents": BASE64.encode(bytes) }))
            }).collect::<EditorResult<Vec<_>>>()?;
            serde_json::to_vec(&json!({ "kind": "keyBundle", "files": bundle }))
                .map_err(display_error)
        });
        let _ = fs::remove_dir_all(&temporary);
        result
    }
}

fn archive_entry_json(entry: &ArchiveEntry) -> Value {
    json!({
        "resource": resource_name(&entry.resref),
        "resref": entry.resref.res_ref(),
        "extension": get_res_ext(entry.resref.res_type()),
        "typeId": entry.resref.res_type().0,
        "size": entry.replacement.as_ref().map(Vec::len).unwrap_or_else(|| entry.original.as_ref().map(|res| res.uncompressed_size()).unwrap_or(0)),
        "modified": entry.replacement.is_some(),
        "compression": format!("{:?}", entry.algorithm),
    })
}

fn archive_page(entries: &[ArchiveEntry], request: &Value) -> (Vec<Value>, usize, usize, usize) {
    let query = request
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let offset = request
        .get("offset")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let limit = page_limit(request);
    let matches = |entry: &&ArchiveEntry| {
        query.is_empty()
            || resource_name(&entry.resref)
                .to_ascii_lowercase()
                .contains(&query)
    };
    let total = entries.iter().filter(matches).count();
    let page = entries
        .iter()
        .filter(matches)
        .skip(offset)
        .take(limit)
        .map(archive_entry_json)
        .collect();
    (page, total, offset, limit)
}

fn resource_name(resref: &ResRef) -> String {
    let extension = get_res_ext(resref.res_type());
    if extension.is_empty() {
        resref.res_ref().to_string()
    } else {
        format!("{}.{}", resref.res_ref(), extension)
    }
}

fn apply_archive_edit(
    entries: &mut Vec<ArchiveEntry>,
    action: &str,
    edit: &Value,
) -> EditorResult<(String, Value)> {
    match action {
        "replaceEntry" => {
            let resource = required_string(edit, "resource")?;
            let bytes = BASE64
                .decode(required_string(edit, "contents")?)
                .map_err(display_error)?;
            let entry = entries
                .iter_mut()
                .find(|entry| resource_name(&entry.resref) == resource)
                .ok_or_else(|| format!("archive resource not found: {resource}"))?;
            let before = entry.replacement.as_ref().map(|bytes| BASE64.encode(bytes));
            entry.replacement = Some(bytes);
            Ok((
                "Replace archive resource".to_string(),
                json!({ "action": "setEntryOverlay", "resource": resource, "contents": before }),
            ))
        }
        "setEntryOverlay" => {
            let resource = required_string(edit, "resource")?;
            let entry = entries
                .iter_mut()
                .find(|entry| resource_name(&entry.resref) == resource)
                .ok_or_else(|| format!("archive resource not found: {resource}"))?;
            let before = entry.replacement.as_ref().map(|bytes| BASE64.encode(bytes));
            entry.replacement = match edit.get("contents") {
                None | Some(Value::Null) => None,
                Some(Value::String(contents)) => Some(
                    BASE64
                        .decode(contents)
                        .map_err(|error| format!("invalid archive payload: {error}"))?,
                ),
                _ => return Err("archive overlay contents must be base64 text or null".to_string()),
            };
            Ok((
                "Restore archive resource".to_string(),
                json!({ "action": "setEntryOverlay", "resource": resource, "contents": before }),
            ))
        }
        "removeEntry" => {
            let resource = required_string(edit, "resource")?;
            let index = entries
                .iter()
                .position(|entry| resource_name(&entry.resref) == resource)
                .ok_or_else(|| format!("archive resource not found: {resource}"))?;
            let entry = entries.remove(index);
            let bytes = entry.bytes()?;
            Ok((
                "Remove archive resource".to_string(),
                json!({ "action": "addEntry", "resource": resource, "contents": BASE64.encode(bytes), "index": index, "algorithm": format!("{:?}", entry.algorithm) }),
            ))
        }
        "addEntry" => {
            let resource = required_string(edit, "resource")?;
            if entries
                .iter()
                .any(|entry| resource_name(&entry.resref).eq_ignore_ascii_case(resource))
            {
                return Err(format!("archive already contains {resource}"));
            }
            let resref = ResolvedResRef::from_filename(resource)
                .map_err(display_error)?
                .base()
                .clone();
            let bytes = BASE64
                .decode(required_string(edit, "contents")?)
                .map_err(display_error)?;
            let index = edit
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|v| usize::try_from(v).ok())
                .unwrap_or(entries.len())
                .min(entries.len());
            let algorithm = edit
                .get("algorithm")
                .and_then(Value::as_str)
                .map(algorithm_from_name)
                .transpose()?
                .unwrap_or(Algorithm::None);
            entries.insert(
                index,
                ArchiveEntry {
                    resref,
                    original: None,
                    replacement: Some(bytes),
                    algorithm,
                },
            );
            Ok((
                "Add archive resource".to_string(),
                json!({ "action": "removeEntry", "resource": resource }),
            ))
        }
        "renameEntry" => {
            let resource = required_string(edit, "resource")?;
            let new_resource = required_string(edit, "newResource")?;
            if entries
                .iter()
                .any(|entry| resource_name(&entry.resref).eq_ignore_ascii_case(new_resource))
            {
                return Err(format!("archive already contains {new_resource}"));
            }
            let entry = entries
                .iter_mut()
                .find(|entry| resource_name(&entry.resref) == resource)
                .ok_or_else(|| format!("archive resource not found: {resource}"))?;
            let old = resource_name(&entry.resref);
            entry.resref = ResolvedResRef::from_filename(new_resource)
                .map_err(display_error)?
                .base()
                .clone();
            Ok((
                "Rename archive resource".to_string(),
                json!({ "action": action, "resource": new_resource, "newResource": old }),
            ))
        }
        _ => Err(format!("unknown archive edit: {action}")),
    }
}

fn algorithm_from_name(value: &str) -> EditorResult<Algorithm> {
    match value.to_ascii_lowercase().as_str() {
        "none" => Ok(Algorithm::None),
        "zlib" => Ok(Algorithm::Zlib),
        "zstd" => Ok(Algorithm::Zstd),
        _ => Err(format!(
            "unsupported archive compression algorithm: {value}"
        )),
    }
}

fn commit_key_staging(staging: &Path, destination: &Path) -> EditorResult<()> {
    let mut relative_files = Vec::new();
    collect_relative_files(staging, staging, &mut relative_files)?;
    let rollback = staging.join(".rollback");
    fs::create_dir(&rollback).map_err(display_error)?;
    let mut committed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for relative in &relative_files {
        let source = staging.join(relative);
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(display_error)?;
        }
        let backup = rollback.join(relative);
        if target.exists() {
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent).map_err(display_error)?;
            }
            fs::rename(&target, &backup).map_err(|error| {
                format!("failed to stage existing {}: {error}", target.display())
            })?;
        }
        if let Err(error) = fs::rename(&source, &target) {
            for (committed_target, committed_backup) in committed.into_iter().rev() {
                let _ = fs::remove_file(&committed_target);
                if committed_backup.exists() {
                    let _ = fs::rename(&committed_backup, &committed_target);
                }
            }
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(format!("failed to commit {}: {error}", target.display()));
        }
        committed.push((target, backup));
    }
    fs::remove_dir_all(staging).map_err(display_error)?;
    Ok(())
}

fn collect_relative_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<PathBuf>,
) -> EditorResult<()> {
    for entry in fs::read_dir(directory).map_err(display_error)? {
        let entry = entry.map_err(display_error)?;
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == ".rollback") {
            continue;
        }
        if entry.file_type().map_err(display_error)?.is_dir() {
            collect_relative_files(root, &path, output)?;
        } else {
            output.push(
                path.strip_prefix(root)
                    .map_err(display_error)?
                    .to_path_buf(),
            );
        }
    }
    output.sort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nwnrs-vscode-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&path).expect("create temporary directory");
        path
    }

    #[test]
    fn gff_merge_preserves_unchanged_fields_and_supports_nested_values() {
        let mut root = GffRoot::new("UTC ");
        root.put_value("Tag", GffValue::CExoString("before".to_string()))
            .expect("insert tag");
        let mut nested = GffStruct::new(7);
        nested
            .put_value("Count", GffValue::Int(1))
            .expect("insert nested value");
        root.put_value("Nested", GffValue::Struct(nested))
            .expect("insert nested struct");

        let mut edited = gff_root_to_json(&root);
        *edited
            .pointer_mut("/root/fields/0/value")
            .expect("tag value") = json!("after");
        *edited
            .pointer_mut("/root/fields/1/value/fields/0/value")
            .expect("nested value") = json!(2);
        merge_gff_root(&mut root, &edited).expect("merge gff");

        assert!(matches!(
            root.root.get_field("Tag").map(GffField::value),
            Some(GffValue::CExoString(value)) if value == "after"
        ));
        let Some(GffValue::Struct(nested)) = root.root.get_field("Nested").map(GffField::value)
        else {
            panic!("nested struct missing");
        };
        assert!(matches!(
            nested.get_field("Count").map(GffField::value),
            Some(GffValue::Int(2))
        ));
    }

    #[test]
    fn dialog_json_documents_remain_canonical_json_after_typed_edits() {
        let directory = temporary_directory("dialog-json");
        let source_path = directory.join("conversation.dlg.json");
        let binary_path = directory.join("conversation.dlg");
        let mut root = GffRoot::new("DLG ");
        root.put_value("EndConverAbort", GffValue::ResRef("before".to_string()))
            .expect("insert dialog field");
        fs::write(
            &source_path,
            gff_root_to_json_bytes(&root).expect("encode dialog JSON"),
        )
        .expect("write dialog JSON");

        let mut document =
            EditorDocument::open(source_path.clone(), false).expect("open dialog JSON document");
        let mut edited = document.snapshot(&json!({})).expect("snapshot dialog")["data"].clone();
        *edited
            .pointer_mut("/root/fields/0/value")
            .expect("dialog field") = json!("after");
        document
            .apply_edit(&json!({
                "edit": { "action": "replaceGff", "root": edited }
            }))
            .expect("edit dialog JSON");
        document.save(&json!({})).expect("save dialog JSON");
        let saved = fs::read(&source_path).expect("read saved dialog JSON");
        let saved_json: Value = serde_json::from_slice(&saved).expect("parse saved dialog JSON");
        assert_eq!(saved_json["__data_type"], "DLG ");
        assert_eq!(saved_json["EndConverAbort"]["value"], "after");

        document
            .save_as(&json!({ "path": binary_path }))
            .expect("save dialog as binary");
        let binary = fs::read(directory.join("conversation.dlg")).expect("read binary dialog");
        assert_eq!(&binary[..4], b"DLG ");
        let reopened = EditorDocument::open(directory.join("conversation.dlg"), false)
            .expect("reopen binary dialog");
        let EditorContent::Gff(reopened) = reopened.content else {
            panic!("binary dialog did not reopen as GFF");
        };
        assert!(matches!(
            reopened.root.root.get_field("EndConverAbort").map(GffField::value),
            Some(GffValue::ResRef(value)) if value == "after"
        ));

        fs::remove_dir_all(directory).expect("remove temporary directory");
    }

    #[test]
    fn erf_editor_replaces_payload_and_roundtrips_archive_metadata() {
        let resource = ResolvedResRef::from_filename("sample.utc")
            .expect("resource reference")
            .base()
            .clone();
        let mut encoded = Cursor::new(Vec::new());
        write_erf_with_options(
            &mut encoded,
            "ERF ",
            ErfVersion::V1,
            2026,
            203,
            ExoResFileCompressionType::None,
            Algorithm::None,
            &BTreeMap::new(),
            -1,
            std::slice::from_ref(&resource),
            None,
            ErfWriteOptions::default(),
            |_resource, output| {
                output.write_all(b"before")?;
                Ok((6, sha1_digest(b"before")))
            },
            |_resource| Algorithm::None,
        )
        .expect("write source erf");
        let archive =
            read_erf(Cursor::new(encoded.into_inner()), "test.erf").expect("read source erf");
        let mut document = ErfDocument::new(archive).expect("create editor document");
        document
            .apply_edit(
                "replaceEntry",
                &json!({
                    "resource": "sample.utc",
                    "contents": BASE64.encode(b"after"),
                }),
            )
            .expect("replace payload");
        let mut rewritten = Cursor::new(Vec::new());
        document.write(&mut rewritten).expect("write edited erf");
        let reparsed =
            read_erf(Cursor::new(rewritten.into_inner()), "test.erf").expect("reparse edited erf");
        assert_eq!(reparsed.build_year, 2026);
        assert_eq!(
            reparsed
                .demand(&resource)
                .expect("resource")
                .read_all(CachePolicy::Bypass)
                .expect("payload"),
            b"after"
        );
    }

    #[test]
    fn key_editor_commits_key_and_bif_as_one_resource_set() {
        let directory = temporary_directory("key");
        let resource = ResolvedResRef::from_filename("sample.utc")
            .expect("resource reference")
            .base()
            .clone();
        let bif = KeyBifEntry {
            directory:         String::new(),
            name:              "demo".to_string(),
            recorded_filename: Some("demo.bif".to_string()),
            drives:            0,
            bif_oid:           None,
            entries:           vec![resource.clone()],
        };
        write_key_and_bif(
            KeyBifVersion::V1,
            ExoResFileCompressionType::None,
            Algorithm::None,
            &directory,
            "demo",
            "",
            &[bif],
            2026,
            203,
            None,
            |_resource, output| {
                output.write_all(b"before")?;
                Ok((6, sha1_digest(b"before")))
            },
        )
        .expect("write key set");

        let table = read_key_table_from_file(directory.join("demo.key")).expect("read key");
        let mut document = KeyDocument::new(table).expect("create key editor");
        document
            .apply_edit(
                "replaceEntry",
                &json!({
                    "resource": "sample.utc",
                    "contents": BASE64.encode(b"after"),
                }),
            )
            .expect("replace key payload");
        document
            .write_atomic(&directory.join("demo.key"))
            .expect("transactional key write");

        let reparsed = read_key_table_from_file(directory.join("demo.key")).expect("reparse key");
        assert_eq!(
            reparsed
                .demand(&resource)
                .expect("resource")
                .read_all(CachePolicy::Bypass)
                .expect("payload"),
            b"after"
        );
        fs::remove_dir_all(directory).expect("remove temporary directory");
    }
}
