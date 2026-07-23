use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io::Cursor,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use napi::{
    Task,
    bindgen_prelude::{AsyncTask, Buffer},
};
use napi_derive::napi;
use nwnrs_nwpkg::{DependencySpec, PROJECT_MANIFEST_FILENAME, read_project_manifest};
use nwnrs_types::{
    gff::{
        ARE_RES_TYPE, GIT_RES_TYPE, IFO_RES_TYPE, gff_root_from_json_bytes, parse_git_root,
        parse_module_info_root, read_gff_root, write_gff_root,
    },
    install::{find_nwnrs_root, find_user_root, new_default_resman, resolve_language_root},
    localization::{BAD_STRREF, CUSTOM_STRREF_OFFSET, Language, resolve_language},
    lru::WeightedLru,
    mdl::{ModelResourceKind, NwnBlueprintKind},
    resman::{CachePolicy, ResContainer, ResMan, ResolvedResRef, read_resdir, read_resmemfile},
    scene::{
        AreaInspectionCache, AreaInspector, InspectionLocalizationResolver,
        InspectionLocalizedEntry, InspectionLocalizedString, SceneAreaObject, SceneDocument,
        SceneLoader, ScenePacket, area_object_catalog,
    },
    tlk::SingleTlk,
};
use serde::{Deserialize, Serialize};

struct ViewerServiceState {
    sessions: Mutex<WeightedLru<String, Arc<Mutex<Option<ViewerSession>>>>>,
}

impl Default for ViewerServiceState {
    fn default() -> Self {
        Self {
            // Each session owns its own resource-manager and decoded-scene
            // caches. Bound the number of inactive packages retained so a
            // long VS Code session cannot grow without limit.
            sessions: Mutex::new(WeightedLru::new(3, 1)),
        }
    }
}

struct ViewerSession {
    configuration_key: String,
    resman:            ResMan,
    scenes:            WeightedLru<String, Arc<CachedViewerScene>>,
    resources:         Option<Arc<Vec<ViewerResourceEntry>>>,
    inspection_cache:  AreaInspectionCache,
    localization:      ViewerLocalization,
}

struct CachedViewerScene {
    scene:       Arc<SceneDocument>,
    catalog:     Arc<Vec<u8>>,
    inspections: Mutex<WeightedLru<String, Arc<String>>>,
}

/// One persistent native viewer service with an independent layered resource
/// session per `nwpkg.toml` project.
#[napi]
pub struct ViewerService {
    state: Arc<ViewerServiceState>,
}

#[napi]
impl ViewerService {
    /// Creates an empty viewer service.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            state: Arc::new(ViewerServiceState::default()),
        }
    }

    /// Assembles and packs one scene away from the JavaScript event loop.
    #[napi]
    pub fn load_scene(&self, request_json: String) -> AsyncTask<ViewerLoadTask> {
        AsyncTask::new(ViewerLoadTask {
            state: Arc::clone(&self.state),
            request_json,
            contents: None,
        })
    }

    /// Assembles a virtual archive entry without copying it through JSON or
    /// base64. The supplied bytes become the highest-precedence resource in
    /// the package session for this load.
    #[napi]
    pub fn load_scene_bytes(
        &self,
        request_json: String,
        contents: Buffer,
    ) -> AsyncTask<ViewerLoadTask> {
        AsyncTask::new(ViewerLoadTask {
            state: Arc::clone(&self.state),
            request_json,
            contents: Some(contents.to_vec()),
        })
    }

    /// Packs one animation selected from an already loaded scene catalog.
    #[napi]
    pub fn load_animation(&self, request_json: String) -> AsyncTask<ViewerAnimationTask> {
        AsyncTask::new(ViewerAnimationTask {
            state: Arc::clone(&self.state),
            request_json,
        })
    }

    /// Packs one texture selected from an already loaded scene catalog.
    #[napi]
    pub fn load_texture(&self, request_json: String) -> AsyncTask<ViewerTextureTask> {
        AsyncTask::new(ViewerTextureTask {
            state: Arc::clone(&self.state),
            request_json,
        })
    }

    /// Lazily builds one complete authored object inspection from a cached
    /// scene.
    #[napi]
    pub fn inspect_area_object(&self, request_json: String) -> AsyncTask<ViewerAreaInspectionTask> {
        AsyncTask::new(ViewerAreaInspectionTask {
            state: Arc::clone(&self.state),
            request_json,
        })
    }

    /// Reads one dependency from the same precedence-aware package session so
    /// VS Code can open installed assets as immutable virtual documents.
    #[napi]
    pub fn read_resource(&self, request_json: String) -> AsyncTask<ViewerReadTask> {
        AsyncTask::new(ViewerReadTask {
            state: Arc::clone(&self.state),
            request_json,
        })
    }

    /// Resolves one dependency through the same package resource graph and
    /// reports the exact winning origin. Directory-backed resources expose
    /// their physical path; packed game resources remain virtual.
    #[napi]
    pub fn resolve_resource(&self, request_json: String) -> AsyncTask<ViewerResolveTask> {
        AsyncTask::new(ViewerResolveTask {
            state: Arc::clone(&self.state),
            request_json,
        })
    }

    /// Reads typed package metadata for one discovered `nwpkg.toml`.
    #[napi]
    pub fn inspect_package(&self, request_json: String) -> AsyncTask<ViewerPackageTask> {
        AsyncTask::new(ViewerPackageTask {
            request_json,
        })
    }

    /// Inspects the authored package source tree and projects the canonical
    /// module-area, dialog, and NWScript sections used by the sidebar.
    #[napi]
    pub fn inspect_package_source(
        &self,
        request_json: String,
    ) -> AsyncTask<ViewerPackageSourceTask> {
        AsyncTask::new(ViewerPackageSourceTask {
            request_json,
        })
    }

    /// Returns one lazy level of the precedence-aware resource catalog.
    #[napi]
    pub fn list_resources(&self, request_json: String) -> AsyncTask<ViewerResourceCatalogTask> {
        AsyncTask::new(ViewerResourceCatalogTask {
            state: Arc::clone(&self.state),
            request_json,
        })
    }

    /// Drops one cached package session, or every session when no key is
    /// supplied. The next load rebuilds the authoritative layered resource
    /// view.
    #[napi]
    pub fn invalidate(&self, session_key: Option<String>) -> napi::Result<()> {
        let mut sessions = self.state.sessions.lock().map_err(|error| {
            napi::Error::from_reason(format!("viewer session map is poisoned: {error}"))
        })?;
        if let Some(session_key) = session_key {
            sessions.remove(&session_key);
        } else {
            sessions.clear();
        }
        Ok(())
    }
}

impl Default for ViewerService {
    fn default() -> Self {
        Self::new()
    }
}

/// Background scene assembly task.
pub struct ViewerLoadTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
    contents:     Option<Vec<u8>>,
}

/// Background dependency read task.
pub struct ViewerReadTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
}

/// Background animation-packet task.
pub struct ViewerAnimationTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
}

/// Background texture-packet task.
pub struct ViewerTextureTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
}

/// Background authored-area-object inspection task.
pub struct ViewerAreaInspectionTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
}

/// Background dependency provenance task.
pub struct ViewerResolveTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
}

/// Background package-manifest inspection task.
pub struct ViewerPackageTask {
    request_json: String,
}

/// Background package-source inspection task.
pub struct ViewerPackageSourceTask {
    request_json: String,
}

/// Background resource-catalog query task.
pub struct ViewerResourceCatalogTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
}

struct ViewerTlkLayer {
    male:            Option<SingleTlk>,
    female:          Option<SingleTlk>,
    male_resource:   Option<String>,
    female_resource: Option<String>,
}

struct ViewerLocalization {
    language:  Language,
    user_root: PathBuf,
    dialog:    ViewerTlkLayer,
    custom:    HashMap<String, ViewerTlkLayer>,
}

impl ViewerLocalization {
    fn new(root: &Path, user_root: PathBuf, language: &str) -> Result<Self, String> {
        let language = resolve_language(language).map_err(|error| error.to_string())?;
        let language_root = resolve_language_root(root, language.short_code())
            .map_err(|error| error.to_string())?;
        let data = language_root.join("data");
        Ok(Self {
            language,
            user_root,
            dialog: ViewerTlkLayer::from_paths(data.join("dialog.tlk"), data.join("dialogf.tlk")),
            custom: HashMap::new(),
        })
    }

    fn ensure_custom(&mut self, resman: &mut ResMan, name: Option<&str>) {
        let Some(name) = name.and_then(valid_custom_tlk_name) else {
            return
        };
        if self.custom.contains_key(name) {
            return;
        }
        let male_name = format!("{name}.tlk");
        let female_name = format!("{name}f.tlk");
        let male_res = ResolvedResRef::from_filename(&male_name)
            .ok()
            .and_then(|resolved| resman.get_resolved(&resolved));
        let female_res = ResolvedResRef::from_filename(&female_name)
            .ok()
            .and_then(|resolved| resman.get_resolved(&resolved));
        let directory = self.user_root.join("tlk");
        self.custom.insert(
            name.to_string(),
            ViewerTlkLayer {
                male_resource:   male_res.as_ref().map_or_else(
                    || {
                        directory
                            .join(&male_name)
                            .is_file()
                            .then_some(male_name.clone())
                    },
                    |res| Some(format!("{} @ {}", male_name, res.origin())),
                ),
                female_resource: female_res.as_ref().map_or_else(
                    || {
                        directory
                            .join(&female_name)
                            .is_file()
                            .then_some(female_name.clone())
                    },
                    |res| Some(format!("{} @ {}", female_name, res.origin())),
                ),
                male:            male_res
                    .as_ref()
                    .and_then(|res| SingleTlk::from_res(res, CachePolicy::Use).ok())
                    .or_else(|| {
                        SingleTlk::from_file(directory.join(&male_name), CachePolicy::Use).ok()
                    }),
                female:          female_res
                    .as_ref()
                    .and_then(|res| SingleTlk::from_res(res, CachePolicy::Use).ok())
                    .or_else(|| {
                        SingleTlk::from_file(directory.join(&female_name), CachePolicy::Use).ok()
                    }),
            },
        );
    }

    fn resolve(
        &mut self,
        value: &nwnrs_types::gff::GffCExoLocString,
        custom: Option<&str>,
    ) -> InspectionLocalizedString {
        let entries = value
            .entries
            .iter()
            .map(|(id, text)| InspectionLocalizedEntry {
                id:   *id,
                text: text.clone(),
            })
            .collect::<Vec<_>>();
        let language_id = i32::try_from(self.language.id()).unwrap_or_default() * 2;
        let inline = value
            .entries
            .iter()
            .find(|(id, text)| *id == language_id && !text.is_empty())
            .or_else(|| {
                value
                    .entries
                    .iter()
                    .find(|(id, text)| *id == language_id + 1 && !text.is_empty())
            })
            .or_else(|| {
                value
                    .entries
                    .iter()
                    .find(|(id, text)| *id == 0 && !text.is_empty())
            })
            .or_else(|| value.entries.iter().find(|(_, text)| !text.is_empty()));
        if let Some((id, text)) = inline {
            return InspectionLocalizedString {
                text: Some(text.clone()),
                str_ref: (value.str_ref != BAD_STRREF).then_some(value.str_ref),
                source: Some("inline".into()),
                language_id: u32::try_from(*id / 2).ok(),
                gender: Some(
                    if id.rem_euclid(2) == 0 {
                        "male"
                    } else {
                        "female"
                    }
                    .into(),
                ),
                entries,
            };
        }
        let mut selected = None;
        if value.str_ref != BAD_STRREF {
            let (layer, index) = if value.str_ref >= CUSTOM_STRREF_OFFSET {
                let name = custom.and_then(valid_custom_tlk_name);
                (
                    name.and_then(|name| self.custom.get_mut(name)),
                    value.str_ref - CUSTOM_STRREF_OFFSET,
                )
            } else {
                (Some(&mut self.dialog), value.str_ref)
            };
            if let Some(layer) = layer {
                selected = layer
                    .male
                    .as_mut()
                    .and_then(|tlk| tlk.get(index).ok().flatten())
                    .map(|entry| (entry.text, layer.male_resource.clone(), "male"))
                    .or_else(|| {
                        layer
                            .female
                            .as_mut()
                            .and_then(|tlk| tlk.get(index).ok().flatten())
                            .map(|entry| (entry.text, layer.female_resource.clone(), "female"))
                    });
            }
        }
        InspectionLocalizedString {
            text: selected.as_ref().map(|(text, _, _)| text.clone()),
            str_ref: (value.str_ref != BAD_STRREF).then_some(value.str_ref),
            source: selected.as_ref().and_then(|(_, source, _)| source.clone()),
            language_id: selected.as_ref().map(|_| self.language.id()),
            gender: selected.map(|(_, _, gender)| gender.into()),
            entries,
        }
    }
}

impl ViewerTlkLayer {
    fn from_paths(male: PathBuf, female: PathBuf) -> Self {
        Self {
            male_resource:   male.is_file().then(|| male.display().to_string()),
            female_resource: female.is_file().then(|| female.display().to_string()),
            male:            SingleTlk::from_file(&male, CachePolicy::Use).ok(),
            female:          SingleTlk::from_file(&female, CachePolicy::Use).ok(),
        }
    }
}

struct ViewerInspectionLocalization<'a> {
    localization: &'a mut ViewerLocalization,
    custom_tlk:   Option<&'a str>,
}

impl InspectionLocalizationResolver for ViewerInspectionLocalization<'_> {
    fn resolve(&mut self, value: &nwnrs_types::gff::GffCExoLocString) -> InspectionLocalizedString {
        self.localization.resolve(value, self.custom_tlk)
    }
}

fn valid_custom_tlk_name(value: &str) -> Option<&str> {
    let value = value
        .get(value.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".tlk"))
        .then(|| value.get(..value.len() - 4))
        .flatten()
        .unwrap_or(value);
    (!value.is_empty()
        && value.len() <= 16
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_'))
    .then_some(value)
}

#[derive(Debug, Deserialize)]
struct ViewerPackageRequest {
    path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerPackageInfo {
    manifest_path:  PathBuf,
    root:           PathBuf,
    name:           String,
    kind:           String,
    source_path:    PathBuf,
    resource_paths: Vec<PathBuf>,
    dependencies:   Vec<ViewerPackageDependency>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerPackageDependency {
    name:          String,
    root:          PathBuf,
    manifest_path: PathBuf,
}

impl Task for ViewerPackageTask {
    type JsValue = String;
    type Output = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerPackageRequest =
            serde_json::from_str(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid package request: {error}"))
            })?;
        let manifest_path = if request.path.is_dir() {
            request.path.join(PROJECT_MANIFEST_FILENAME)
        } else {
            request.path
        };
        let root = manifest_path
            .parent()
            .ok_or_else(|| napi::Error::from_reason("package manifest has no parent directory"))?
            .to_path_buf();
        let manifest = read_project_manifest(&manifest_path)
            .map_err(napi::Error::from_reason)?
            .ok_or_else(|| {
                napi::Error::from_reason(format!(
                    "{} is not an nwnrs package",
                    manifest_path.display()
                ))
            })?;
        let dependencies = manifest
            .dependencies
            .iter()
            .map(|(name, dependency)| {
                let DependencySpec::Path(dependency) = dependency;
                let dependency_root = root.join(&dependency.path);
                ViewerPackageDependency {
                    name:          name.clone(),
                    manifest_path: dependency_root.join(PROJECT_MANIFEST_FILENAME),
                    root:          dependency_root,
                }
            })
            .collect();
        let resource_paths = package_resource_roots(&root)
            .map_err(napi::Error::from_reason)?
            .dependencies
            .into_iter()
            .chain(std::iter::once(root.join(&manifest.source.path)))
            .collect();
        serde_json::to_string(&ViewerPackageInfo {
            manifest_path,
            source_path: root.join(&manifest.source.path),
            resource_paths,
            root,
            name: manifest.project.name,
            kind: manifest.project.kind.to_string(),
            dependencies,
        })
        .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewerPackageSourceRequest {
    manifest_path: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerPackageSourceInfo {
    source_path: PathBuf,
    areas:       Vec<ViewerPackageSourceArea>,
    dialogs:     Vec<ViewerPackageSourceFile>,
    code:        Vec<ViewerPackageSourceFile>,
    warnings:    Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerPackageSourceArea {
    resref:       String,
    registered:   bool,
    files:        Vec<ViewerPackageSourceFile>,
    missing:      Vec<String>,
    conflicts:    Vec<String>,
    objects:      Vec<SceneAreaObject>,
    object_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerPackageSourceFile {
    path:          PathBuf,
    relative_path: String,
    kind:          String,
}

impl Task for ViewerPackageSourceTask {
    type JsValue = String;
    type Output = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerPackageSourceRequest = serde_json::from_str(&self.request_json)
            .map_err(|error| {
                napi::Error::from_reason(format!("invalid package-source request: {error}"))
            })?;
        let result =
            inspect_package_source(&request.manifest_path).map_err(napi::Error::from_reason)?;
        serde_json::to_string(&result).map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

fn inspect_package_source(manifest_path: &Path) -> Result<ViewerPackageSourceInfo, String> {
    let root = manifest_path
        .parent()
        .ok_or_else(|| "package manifest has no parent directory".to_string())?;
    let manifest = read_project_manifest(manifest_path)?
        .ok_or_else(|| format!("{} is not an nwnrs package", manifest_path.display()))?;
    let source_path = root.join(manifest.source.path);
    let source_path = source_path
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", source_path.display()))?;
    let mut paths = Vec::new();
    let mut warnings = Vec::new();
    collect_source_paths(
        &source_path,
        &mut BTreeSet::new(),
        &mut paths,
        &mut warnings,
    )?;
    paths.sort_by(|left, right| {
        source_relative_path(&source_path, left)
            .to_ascii_lowercase()
            .cmp(&source_relative_path(&source_path, right).to_ascii_lowercase())
            .then(left.cmp(right))
    });

    let mut dialogs = Vec::new();
    let mut code = Vec::new();
    let mut area_files = BTreeMap::<String, (String, Vec<ViewerPackageSourceFile>)>::new();
    let mut module_ifos = Vec::new();
    for path in paths {
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower_name = file_name.to_ascii_lowercase();
        if lower_name == "module.ifo" || lower_name == "module.ifo.json" {
            module_ifos.push(path.clone());
        }
        if lower_name.ends_with(".nss") && lower_name != "nwscript.nss" {
            code.push(source_file(&source_path, &path, "nss"));
            continue;
        }
        let Some((resref, extension, json_source)) = compound_resource_identity(file_name) else {
            continue;
        };
        match extension.as_str() {
            "dlg" => dialogs.push(source_file(
                &source_path,
                &path,
                if json_source { "dlgJson" } else { "dlg" },
            )),
            "are" | "git" | "gic" => {
                let entry = area_files
                    .entry(resref.to_ascii_lowercase())
                    .or_insert_with(|| (resref, Vec::new()));
                entry.1.push(source_file(&source_path, &path, &extension));
            }
            _ => {}
        }
    }

    module_ifos.sort_by_key(|path| {
        let json_source = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.to_ascii_lowercase().ends_with(".json"));
        (!json_source, path.clone())
    });
    if module_ifos.len() > 1 {
        warnings.push(format!(
            "multiple module IFO sources found; using {}",
            module_ifos[0].display()
        ));
    }
    let registered = if let Some(path) = module_ifos.first() {
        match read_module_area_resrefs(path) {
            Ok(areas) => areas,
            Err(error) => {
                warnings.push(error);
                Vec::new()
            }
        }
    } else {
        warnings.push("module.ifo or module.ifo.json was not found".to_string());
        Vec::new()
    };

    let mut seen = BTreeSet::new();
    let mut areas = Vec::new();
    for resref in registered {
        let key = resref.to_ascii_lowercase();
        if !seen.insert(key.clone()) {
            warnings.push(format!("module IFO declares area {resref} more than once"));
            continue;
        }
        let files = area_files
            .remove(&key)
            .map_or_else(Vec::new, |(_, files)| files);
        areas.push(source_area(resref, true, files));
    }
    areas.extend(
        area_files
            .into_values()
            .map(|(resref, files)| source_area(resref, false, files)),
    );
    dialogs.sort_by(source_file_order);
    code.sort_by(source_file_order);

    Ok(ViewerPackageSourceInfo {
        source_path,
        areas,
        dialogs,
        code,
        warnings,
    })
}

fn collect_source_paths(
    directory: &Path,
    visited: &mut BTreeSet<PathBuf>,
    output: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let canonical = directory
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", directory.display()))?;
    if !visited.insert(canonical) {
        return Ok(());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
    {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(error) => warnings.push(format!(
                "failed to read an entry in {}: {error}",
                directory.display()
            )),
        }
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                warnings.push(format!("failed to inspect {}: {error}", path.display()));
                continue;
            }
        };
        if metadata.is_dir() {
            if matches!(
                path.file_name().and_then(|value| value.to_str()),
                Some(".git" | ".svn")
            ) {
                continue;
            }
            collect_source_paths(&path, visited, output, warnings)?;
        } else if metadata.is_file() {
            output.push(path);
        }
    }
    Ok(())
}

fn compound_resource_identity(file_name: &str) -> Option<(String, String, bool)> {
    let lower_name = file_name.to_ascii_lowercase();
    let (resource_name, json_source) = lower_name
        .strip_suffix(".json")
        .map_or((lower_name.as_str(), false), |value| (value, true));
    let resource = Path::new(resource_name);
    let extension = resource.extension()?.to_str()?.to_ascii_lowercase();
    let resref = resource.file_stem()?.to_str()?.to_string();
    Some((resref, extension, json_source))
}

fn source_file(source_root: &Path, path: &Path, kind: &str) -> ViewerPackageSourceFile {
    ViewerPackageSourceFile {
        path:          path.to_path_buf(),
        relative_path: source_relative_path(source_root, path),
        kind:          kind.to_string(),
    }
}

fn source_relative_path(source_root: &Path, path: &Path) -> String {
    path.strip_prefix(source_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn source_file_order(
    left: &ViewerPackageSourceFile,
    right: &ViewerPackageSourceFile,
) -> std::cmp::Ordering {
    left.relative_path
        .to_ascii_lowercase()
        .cmp(&right.relative_path.to_ascii_lowercase())
        .then(left.relative_path.cmp(&right.relative_path))
}

fn source_area(
    resref: String,
    registered: bool,
    mut files: Vec<ViewerPackageSourceFile>,
) -> ViewerPackageSourceArea {
    files.sort_by(|left, right| {
        area_component_order(&left.kind)
            .cmp(&area_component_order(&right.kind))
            .then_with(|| source_file_order(left, right))
    });
    let mut counts = BTreeMap::<&str, usize>::new();
    for file in &files {
        *counts.entry(file.kind.as_str()).or_default() += 1;
    }
    let missing = ["are", "git", "gic"]
        .into_iter()
        .filter(|kind| !counts.contains_key(kind))
        .map(|kind| kind.to_ascii_uppercase())
        .collect();
    let conflicts = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(kind, _)| kind.to_ascii_uppercase())
        .collect();
    let (objects, object_error) = read_source_area_objects(&files)
        .map(|objects| (objects, None))
        .unwrap_or_else(|error| (Vec::new(), Some(error)));
    ViewerPackageSourceArea {
        resref,
        registered,
        files,
        missing,
        conflicts,
        objects,
        object_error,
    }
}

fn read_source_area_objects(
    files: &[ViewerPackageSourceFile],
) -> Result<Vec<SceneAreaObject>, String> {
    let git_files = files
        .iter()
        .filter(|file| file.kind.eq_ignore_ascii_case("git"))
        .collect::<Vec<_>>();
    match git_files.as_slice() {
        [] => Ok(Vec::new()),
        [file] => {
            let bytes = encode_gff_source(&file.path, "GIT")?;
            let root = read_gff_root(&mut Cursor::new(bytes)).map_err(|error| {
                format!(
                    "failed to parse authored objects from {}: {error}",
                    file.path.display()
                )
            })?;
            let git = parse_git_root(&root).map_err(|error| {
                format!(
                    "failed to inspect authored objects in {}: {error}",
                    file.path.display()
                )
            })?;
            Ok(area_object_catalog(&git))
        }
        _ => Err("authored objects are unavailable while multiple GIT sources conflict".into()),
    }
}

fn area_component_order(kind: &str) -> usize {
    match kind {
        "are" => 0,
        "git" => 1,
        "gic" => 2,
        _ => 3,
    }
}

fn read_module_area_resrefs(path: &Path) -> Result<Vec<String>, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let json_source = path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.to_ascii_lowercase().ends_with(".json"));
    let root = if json_source {
        gff_root_from_json_bytes(bytes)
    } else {
        read_gff_root(&mut Cursor::new(bytes))
    }
    .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    parse_module_info_root(&root)
        .map(|info| info.areas)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))
}

#[derive(Debug, Clone)]
struct ViewerResourceEntry {
    resource:  String,
    extension: String,
    family:    String,
    layer:     String,
    origin:    String,
    file_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewerResourceCatalogRequest {
    #[serde(flatten)]
    viewer:    ViewerLoadRequest,
    stage:     String,
    #[serde(default)]
    layer:     Option<String>,
    #[serde(default)]
    family:    Option<String>,
    #[serde(default)]
    extension: Option<String>,
    #[serde(default)]
    prefix:    String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerResourceCatalogResponse {
    items: Vec<ViewerResourceCatalogItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ViewerResourceCatalogItem {
    kind:      String,
    label:     String,
    count:     usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    layer:     Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    family:    Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prefix:    Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource:  Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin:    Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct ViewerResolvedResource {
    resource:  String,
    origin:    String,
    file_path: Option<PathBuf>,
}

impl Task for ViewerResolveTask {
    type JsValue = String;
    type Output = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerLoadRequest =
            serde_json::from_str(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid viewer request: {error}"))
            })?;
        let session = package_session(&self.state, &request)?;
        let mut session = session.lock().map_err(|error| {
            napi::Error::from_reason(format!("viewer package session is poisoned: {error}"))
        })?;
        ensure_session(&mut session, &request)?;
        let session = session
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("viewer session was not initialized"))?;
        let filename = request
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| napi::Error::from_reason("resource path has no UTF-8 filename"))?;
        let resolved = ResolvedResRef::from_filename(filename)
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        let resource = session
            .resman
            .get_resolved(&resolved)
            .ok_or_else(|| napi::Error::from_reason(format!("resource not found: {resolved}")))?;
        let origin = resource.origin();
        let label_path = PathBuf::from(origin.label());
        let result = ViewerResolvedResource {
            resource:  resolved.to_string(),
            origin:    origin.to_string(),
            file_path: (origin.container().starts_with("ResDir:") && label_path.is_file())
                .then_some(label_path),
        };
        serde_json::to_string(&result).map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

impl Task for ViewerReadTask {
    type JsValue = Buffer;
    type Output = Vec<u8>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerLoadRequest =
            serde_json::from_str(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid viewer request: {error}"))
            })?;
        let session = package_session(&self.state, &request)?;
        let mut session = session.lock().map_err(|error| {
            napi::Error::from_reason(format!("viewer package session is poisoned: {error}"))
        })?;
        ensure_session(&mut session, &request)?;
        let session = session
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("viewer session was not initialized"))?;
        let filename = request
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| napi::Error::from_reason("resource path has no UTF-8 filename"))?;
        let resolved = ResolvedResRef::from_filename(filename)
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        let resource = session
            .resman
            .get_resolved(&resolved)
            .ok_or_else(|| napi::Error::from_reason(format!("resource not found: {resolved}")))?;
        resource
            .read_all(nwnrs_types::resman::CachePolicy::Use)
            .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

impl Task for ViewerResourceCatalogTask {
    type JsValue = String;
    type Output = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerResourceCatalogRequest = serde_json::from_str(&self.request_json)
            .map_err(|error| {
                napi::Error::from_reason(format!("invalid resource catalog request: {error}"))
            })?;
        let package = package_session(&self.state, &request.viewer)?;
        let mut package = package.lock().map_err(|error| {
            napi::Error::from_reason(format!("viewer package session is poisoned: {error}"))
        })?;
        ensure_session(&mut package, &request.viewer)?;
        let session = package
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("viewer session was not initialized"))?;
        if session.resources.is_none() {
            session.resources = Some(Arc::new(build_resource_catalog(
                &session.resman,
                &request.viewer,
            )));
        }
        let resources = session.resources.as_deref().ok_or_else(|| {
            napi::Error::from_reason("viewer resource catalog was not initialized")
        })?;
        let items = query_resource_catalog(resources, &request)?;
        serde_json::to_string(&ViewerResourceCatalogResponse {
            items,
        })
        .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

fn build_resource_catalog(
    resman: &ResMan,
    request: &ViewerLoadRequest,
) -> Vec<ViewerResourceEntry> {
    let mut references = resman.contents().into_iter().collect::<Vec<_>>();
    references.sort_by(|left, right| {
        left.res_ref()
            .to_ascii_lowercase()
            .cmp(&right.res_ref().to_ascii_lowercase())
            .then(left.res_type().cmp(&right.res_type()))
    });
    let package_roots = package_resource_roots(&request.project_root).unwrap_or_default();
    let installation_root = request.root.clone().or_else(|| find_nwnrs_root("").ok());
    let override_root = installation_root.as_ref().map(|root| root.join("ovr"));
    let archive_roots = request
        .archives
        .iter()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
        .collect::<Vec<_>>();
    let mut result = Vec::with_capacity(references.len());
    for reference in references {
        let Some(resolved) = reference.resolve() else {
            continue;
        };
        let Some(resource) = resman
            .containers()
            .iter()
            .find(|container| container.contains(&reference))
            .and_then(|container| container.demand(&reference).ok())
        else {
            continue;
        };
        let origin = resource.origin();
        let file_path = PathBuf::from(origin.label());
        let layer = classify_resource_layer(
            origin.container(),
            &file_path,
            request,
            &package_roots,
            override_root.as_deref(),
            &archive_roots,
        );
        let extension = resolved.res_ext().to_ascii_lowercase();
        result.push(ViewerResourceEntry {
            resource: resolved.to_file(),
            family: resource_family(&extension).into(),
            extension,
            layer: layer.into(),
            origin: origin.to_string(),
            file_path: (origin.container().starts_with("ResDir:") && file_path.is_file())
                .then_some(file_path),
        });
    }
    result
}

#[derive(Default)]
struct PackageResourceRoots {
    workspace:    Vec<PathBuf>,
    dependencies: Vec<PathBuf>,
}

fn package_resource_roots(project_root: &Path) -> Result<PackageResourceRoots, String> {
    let normalized = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let Some(manifest) = read_project_manifest(&normalized)? else {
        return Ok(PackageResourceRoots {
            workspace:    vec![normalized],
            dependencies: Vec::new(),
        });
    };
    let mut dependencies = Vec::new();
    let mut visited = BTreeSet::new();
    for dependency in manifest.dependencies.values() {
        let DependencySpec::Path(dependency) = dependency;
        collect_project_resource_directories(
            &normalized.join(&dependency.path),
            &mut visited,
            &mut dependencies,
        )?;
    }
    Ok(PackageResourceRoots {
        workspace: vec![normalized.join(manifest.source.path)],
        dependencies,
    })
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(root)
}

fn classify_resource_layer(
    container: &str,
    file_path: &Path,
    request: &ViewerLoadRequest,
    package_roots: &PackageResourceRoots,
    override_root: Option<&Path>,
    archives: &[PathBuf],
) -> &'static str {
    if container.starts_with("KeyTable:") || container.starts_with("ResNWSync") {
        return "Vanilla";
    }
    if container.starts_with("Erf:")
        || archives
            .iter()
            .any(|archive| container.contains(archive.to_string_lossy().as_ref()))
    {
        return "Archives";
    }
    if override_root.is_some_and(|root| path_is_within(file_path, root)) {
        return "User Override";
    }
    if package_roots
        .workspace
        .iter()
        .any(|root| path_is_within(file_path, root))
        || path_is_within(file_path, &request.project_root)
            && !package_roots
                .dependencies
                .iter()
                .any(|root| path_is_within(file_path, root))
    {
        return "Workspace";
    }
    if package_roots
        .dependencies
        .iter()
        .any(|root| path_is_within(file_path, root))
    {
        return "Package Dependencies";
    }
    "Vanilla"
}

fn resource_family(extension: &str) -> &'static str {
    match extension {
        "mdl" | "wok" | "dwk" | "pwk" | "plh" => "Models",
        "dds" | "tga" | "plt" | "txi" | "mtr" | "tex" | "bmp" | "ktx" | "png" | "jpg" => "Textures",
        "nss" | "ncs" | "lua" => "Scripts",
        "utc" | "utd" | "ute" | "uti" | "utm" | "utp" | "uts" | "utt" | "utw" | "utg" => {
            "Blueprints"
        }
        "2da" | "tlk" | "ids" => "Tables",
        "wav" | "bmu" | "mpg" | "mve" | "wfx" | "bik" | "wbm" => "Audio",
        "are" | "git" | "gic" | "ifo" | "mod" | "nwm" | "sav" => "Areas & Modules",
        "dlg" | "gui" | "jui" => "Dialogs & UI",
        "erf" | "hak" | "key" | "bif" => "Archives",
        _ => "Other",
    }
}

fn query_resource_catalog(
    resources: &[ViewerResourceEntry],
    request: &ViewerResourceCatalogRequest,
) -> napi::Result<Vec<ViewerResourceCatalogItem>> {
    let filtered = resources
        .iter()
        .filter(|entry| {
            request
                .layer
                .as_ref()
                .is_none_or(|value| &entry.layer == value)
        })
        .filter(|entry| {
            request
                .family
                .as_ref()
                .is_none_or(|value| &entry.family == value)
        })
        .filter(|entry| {
            request
                .extension
                .as_ref()
                .is_none_or(|value| &entry.extension == value)
        })
        .collect::<Vec<_>>();
    match request.stage.as_str() {
        "layers" => Ok(group_catalog(
            &filtered,
            |entry| entry.layer.clone(),
            "layer",
        )),
        "families" => Ok(group_catalog(
            &filtered,
            |entry| entry.family.clone(),
            "family",
        )),
        "types" => Ok(group_catalog(
            &filtered,
            |entry| entry.extension.to_ascii_uppercase(),
            "extension",
        )),
        "names" => Ok(name_catalog(&filtered, &request.prefix)),
        stage => Err(napi::Error::from_reason(format!(
            "unsupported resource catalog stage: {stage}"
        ))),
    }
}

fn group_catalog(
    resources: &[&ViewerResourceEntry],
    key: impl Fn(&ViewerResourceEntry) -> String,
    kind: &str,
) -> Vec<ViewerResourceCatalogItem> {
    let mut groups = BTreeMap::<String, usize>::new();
    for resource in resources {
        *groups.entry(key(resource)).or_default() += 1;
    }
    groups
        .into_iter()
        .map(|(label, count)| ViewerResourceCatalogItem {
            kind: kind.into(),
            label: label.clone(),
            count,
            layer: (kind == "layer").then_some(label.clone()),
            family: (kind == "family").then_some(label.clone()),
            extension: (kind == "extension").then_some(label.to_ascii_lowercase()),
            prefix: None,
            resource: None,
            origin: None,
            file_path: None,
        })
        .collect()
}

fn name_catalog(
    resources: &[&ViewerResourceEntry],
    prefix: &str,
) -> Vec<ViewerResourceCatalogItem> {
    const MAX_LEAVES: usize = 200;
    let prefix = prefix.to_ascii_lowercase();
    let matching = resources
        .iter()
        .copied()
        .filter(|entry| entry.resource.to_ascii_lowercase().starts_with(&prefix))
        .collect::<Vec<_>>();
    if matching.len() <= MAX_LEAVES {
        return matching
            .into_iter()
            .map(|entry| ViewerResourceCatalogItem {
                kind:      "resource".into(),
                label:     entry.resource.clone(),
                count:     1,
                layer:     Some(entry.layer.clone()),
                family:    Some(entry.family.clone()),
                extension: Some(entry.extension.clone()),
                prefix:    None,
                resource:  Some(entry.resource.clone()),
                origin:    Some(entry.origin.clone()),
                file_path: entry.file_path.clone(),
            })
            .collect();
    }
    let mut exact = Vec::new();
    let mut groups = BTreeMap::<String, usize>::new();
    for entry in matching {
        let resource = entry.resource.to_ascii_lowercase();
        let base = resource
            .strip_suffix(&format!(".{}", entry.extension))
            .unwrap_or(&resource);
        let mut characters = base.chars();
        for _ in 0..prefix.chars().count() {
            let _ = characters.next();
        }
        if let Some(next) = characters.next() {
            let child = format!("{prefix}{next}");
            *groups.entry(child).or_default() += 1;
        } else {
            exact.push(entry);
        }
    }
    let mut result = exact
        .into_iter()
        .map(|entry| ViewerResourceCatalogItem {
            kind:      "resource".into(),
            label:     entry.resource.clone(),
            count:     1,
            layer:     Some(entry.layer.clone()),
            family:    Some(entry.family.clone()),
            extension: Some(entry.extension.clone()),
            prefix:    None,
            resource:  Some(entry.resource.clone()),
            origin:    Some(entry.origin.clone()),
            file_path: entry.file_path.clone(),
        })
        .collect::<Vec<_>>();
    result.extend(
        groups
            .into_iter()
            .map(|(child, count)| ViewerResourceCatalogItem {
                kind: "prefix".into(),
                label: child.to_ascii_uppercase(),
                count,
                layer: None,
                family: None,
                extension: None,
                prefix: Some(child),
                resource: None,
                origin: None,
                file_path: None,
            }),
    );
    result
}

impl Task for ViewerLoadTask {
    type JsValue = Buffer;
    type Output = Vec<u8>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerLoadRequest =
            serde_json::from_str(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid viewer request: {error}"))
            })?;
        let session = package_session(&self.state, &request)?;
        let mut session = session.lock().map_err(|error| {
            napi::Error::from_reason(format!("viewer package session is poisoned: {error}"))
        })?;
        ensure_session(&mut session, &request)?;
        let session = session
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("viewer session was not initialized"))?;
        let filename = request
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| napi::Error::from_reason("viewer path has no UTF-8 filename"))?;
        let resource = ResolvedResRef::from_filename(filename)
            .map_err(|error| napi::Error::from_reason(format!("viewer resource name: {error}")))?;
        let overlay_sources = if let Some(authored) = &request.authored_area {
            authored_area_overlay_sources(authored)?
        } else if let Some(contents) = self.contents.take() {
            vec![(filename.to_string(), contents)]
        } else {
            Vec::new()
        };
        let cache_key = scene_cache_key(&request, &resource, &overlay_sources);
        if let Some(cached) = session.scenes.get(&cache_key) {
            return Ok(cached.catalog.as_ref().clone());
        }
        let overlays = overlay_sources
            .into_iter()
            .map(|(overlay_filename, contents)| {
                let resolved =
                    ResolvedResRef::from_filename(&overlay_filename).map_err(|error| {
                        napi::Error::from_reason(format!(
                            "virtual viewer resource {overlay_filename}: {error}"
                        ))
                    })?;
                let memory = read_resmemfile(&overlay_filename, resolved.into(), contents)
                    .map_err(|error| {
                        napi::Error::from_reason(format!(
                            "virtual viewer resource {overlay_filename}: {error}"
                        ))
                    })?;
                Ok(Arc::new(memory) as Arc<dyn ResContainer>)
            })
            .collect::<napi::Result<Vec<_>>>()?;
        for overlay in &overlays {
            session.resman.add(Arc::clone(overlay));
        }
        let loaded = {
            let mut loader = SceneLoader::new(&mut session.resman);
            if ModelResourceKind::from_res_type(resource.base().res_type()).is_some() {
                loader.load_model(&resource)
            } else if NwnBlueprintKind::from_res_type(resource.base().res_type()).is_some() {
                loader.load_blueprint(&resource)
            } else if matches!(resource.base().res_type(), ARE_RES_TYPE | GIT_RES_TYPE) {
                loader.load_area(resource.base().res_ref())
            } else if resource.base().res_type() == IFO_RES_TYPE {
                loader.load_module_area(resource.base().res_ref(), request.area.as_deref())
            } else {
                Err(nwnrs_types::scene::SceneError::invalid(format!(
                    "{} does not have a 3D scene provider",
                    resource
                )))
            }
            .map_err(|error| napi::Error::from_reason(error.to_string()))
        };
        for overlay in &overlays {
            session.resman.remove(overlay);
        }
        let scene = Arc::new(loaded?);
        let mut packet = ScenePacket::catalog_from_scene(&scene)
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        packet.manifest.asset_key = Some(cache_key.clone());
        let packet = packet
            .encode()
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        let weight = packet
            .len()
            .saturating_add(
                scene
                    .textures
                    .iter()
                    .map(|texture| {
                        texture.rgba8.len()
                            + texture
                                .compressed
                                .as_ref()
                                .map(|compressed| {
                                    compressed
                                        .mip_levels
                                        .iter()
                                        .map(|mip| mip.data.len())
                                        .sum::<usize>()
                                })
                                .unwrap_or(0)
                    })
                    .sum::<usize>(),
            )
            .max(1);
        session.scenes.insert_weighted(
            cache_key,
            weight,
            Arc::new(CachedViewerScene {
                scene,
                catalog: Arc::new(packet.clone()),
                inspections: Mutex::new(WeightedLru::new(8 * 1024 * 1024, 1)),
            }),
        );
        Ok(packet)
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

impl Task for ViewerAnimationTask {
    type JsValue = Buffer;
    type Output = Vec<u8>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerAssetRequest =
            serde_json::from_str(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid viewer asset request: {error}"))
            })?;
        let scene = cached_scene(&self.state, &request)?;
        let mut packet = ScenePacket::animation_from_scene(
            &scene,
            request.model_index.ok_or_else(|| {
                napi::Error::from_reason("animation request is missing modelIndex")
            })?,
            request.animation_index.ok_or_else(|| {
                napi::Error::from_reason("animation request is missing animationIndex")
            })?,
        )
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        packet.manifest.asset_key = Some(request.asset_key);
        packet
            .encode()
            .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

impl Task for ViewerTextureTask {
    type JsValue = Buffer;
    type Output = Vec<u8>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerAssetRequest =
            serde_json::from_str(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid viewer asset request: {error}"))
            })?;
        let scene = cached_scene(&self.state, &request)?;
        let mut packet = ScenePacket::texture_from_scene(
            &scene,
            request.texture_index.ok_or_else(|| {
                napi::Error::from_reason("texture request is missing textureIndex")
            })?,
            request.prefer_compressed,
        )
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        packet.manifest.asset_key = Some(request.asset_key);
        packet
            .encode()
            .map_err(|error| napi::Error::from_reason(error.to_string()))
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

impl Task for ViewerAreaInspectionTask {
    type JsValue = String;
    type Output = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let request: ViewerAreaInspectionRequest = serde_json::from_str(&self.request_json)
            .map_err(|error| {
                napi::Error::from_reason(format!("invalid area inspection request: {error}"))
            })?;
        let session = {
            let mut sessions = self.state.sessions.lock().map_err(|error| {
                napi::Error::from_reason(format!("viewer session map is poisoned: {error}"))
            })?;
            Arc::clone(sessions.get(&request.session_key).ok_or_else(|| {
                napi::Error::from_reason("viewer session is no longer available; reload the scene")
            })?)
        };
        let mut session = session.lock().map_err(|error| {
            napi::Error::from_reason(format!("viewer package session is poisoned: {error}"))
        })?;
        let session = session
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("viewer session was not initialized"))?;
        let cached = Arc::clone(session.scenes.get(&request.asset_key).ok_or_else(|| {
            napi::Error::from_reason("viewer scene assets were evicted; reload the scene")
        })?);
        {
            let mut inspections = cached.inspections.lock().map_err(|error| {
                napi::Error::from_reason(format!("inspection cache is poisoned: {error}"))
            })?;
            if let Some(encoded) = inspections.get(&request.object_key).cloned() {
                return Ok(encoded.as_ref().clone());
            }
        }

        let custom_tlk = cached
            .scene
            .module
            .as_ref()
            .and_then(|module| module.custom_tlk.as_deref());
        let ViewerSession {
            resman,
            inspection_cache,
            localization,
            ..
        } = session;
        localization.ensure_custom(resman, custom_tlk);
        let mut localization = ViewerInspectionLocalization {
            localization,
            custom_tlk,
        };
        let inspection = AreaInspector::new(resman, inspection_cache, &mut localization)
            .inspect(&cached.scene, &request.object_key)
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        let encoded = Arc::new(serde_json::to_string(&inspection).map_err(|error| {
            napi::Error::from_reason(format!("failed to encode area inspection: {error}"))
        })?);
        cached
            .inspections
            .lock()
            .map_err(|error| {
                napi::Error::from_reason(format!("inspection cache is poisoned: {error}"))
            })?
            .insert_weighted(
                request.object_key,
                encoded.len().max(1),
                Arc::clone(&encoded),
            );
        Ok(encoded.as_ref().clone())
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ViewerLoadRequest {
    session_key: String,
    path: PathBuf,
    project_root: PathBuf,
    #[serde(default)]
    area: Option<String>,
    #[serde(default)]
    authored_area: Option<ViewerAuthoredAreaRequest>,
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default)]
    user: Option<PathBuf>,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    load_ovr: bool,
    #[serde(default)]
    archives: Vec<PathBuf>,
    #[serde(default = "default_true")]
    include_project_resources: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ViewerAuthoredAreaRequest {
    resref: String,
    are:    PathBuf,
    git:    PathBuf,
    #[serde(default)]
    gic:    Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewerAssetRequest {
    session_key:       String,
    asset_key:         String,
    #[serde(default)]
    model_index:       Option<usize>,
    #[serde(default)]
    animation_index:   Option<usize>,
    #[serde(default)]
    texture_index:     Option<usize>,
    #[serde(default)]
    prefer_compressed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewerAreaInspectionRequest {
    session_key: String,
    asset_key:   String,
    object_key:  String,
}

#[derive(Serialize)]
struct ViewerConfiguration<'a> {
    project_root: &'a Path,
    root: &'a Option<PathBuf>,
    user: &'a Option<PathBuf>,
    language: &'a str,
    load_ovr: bool,
    resource_dir: Option<&'a Path>,
    archives: &'a [PathBuf],
    include_project_resources: bool,
}

impl ViewerLoadRequest {
    fn configuration(&self) -> ViewerConfiguration<'_> {
        ViewerConfiguration {
            project_root: &self.project_root,
            root: &self.root,
            user: &self.user,
            language: &self.language,
            load_ovr: self.load_ovr,
            resource_dir: self.path.parent(),
            archives: &self.archives,
            include_project_resources: self.include_project_resources,
        }
    }
}

fn build_session(
    request: &ViewerLoadRequest,
    configuration_key: String,
) -> Result<ViewerSession, String> {
    let root = request.root.clone().map_or_else(
        || find_nwnrs_root("").map_err(|error| error.to_string()),
        Ok,
    )?;
    let user = request
        .user
        .clone()
        .map_or_else(|| find_user_root("").map_err(|error| error.to_string()), Ok)?;
    let mut directories = if request.include_project_resources {
        project_resource_directories(&request.project_root)?
    } else {
        Vec::new()
    };
    if request.include_project_resources
        && let Some(parent) = request.path.parent()
    {
        push_unique_path(&mut directories, parent.to_path_buf());
    }
    let localization = ViewerLocalization::new(&root, user.clone(), &request.language)?;
    let mut resman = new_default_resman(
        &root,
        &user,
        &request.language,
        96,
        true,
        request.load_ovr,
        &[],
        &request.archives,
        &[],
        &[],
    )
    .map_err(|error| error.to_string())?;
    for directory in &directories {
        resman.add(
            Arc::new(read_resdir(directory).map_err(|error| error.to_string())?)
                as Arc<dyn ResContainer>,
        );
        for (filename, contents) in gff_json_resources(directory)? {
            let resource = ResolvedResRef::from_filename(&filename)
                .map_err(|error| format!("invalid GFF JSON resource {filename}: {error}"))?;
            let memory = read_resmemfile(&filename, resource.into(), contents)
                .map_err(|error| format!("failed to load GFF JSON resource {filename}: {error}"))?;
            resman.add(Arc::new(memory) as Arc<dyn ResContainer>);
        }
    }
    Ok(ViewerSession {
        configuration_key,
        resman,
        scenes: WeightedLru::new(96 * 1024 * 1024, 1),
        resources: None,
        inspection_cache: AreaInspectionCache::default(),
        localization,
    })
}

fn gff_json_resources(directory: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut paths = Vec::new();
    collect_source_paths(directory, &mut BTreeSet::new(), &mut paths, &mut Vec::new())?;
    paths.sort();
    let mut resources = BTreeMap::new();
    for path in paths {
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = file_name.to_ascii_lowercase();
        let Some(resource_name) = lower.strip_suffix(".json") else {
            continue;
        };
        let Some(extension) = Path::new(resource_name)
            .extension()
            .and_then(|value| value.to_str())
        else {
            continue;
        };
        if !is_gff_resource_extension(extension) {
            continue;
        }
        if ResolvedResRef::from_filename(resource_name).is_err() {
            continue;
        }
        if resources.contains_key(resource_name) {
            return Err(format!(
                "duplicate authored resource {resource_name} in {}",
                directory.display()
            ));
        }
        resources.insert(
            resource_name.to_string(),
            encode_gff_source(&path, extension)?,
        );
    }
    Ok(resources.into_iter().collect())
}

fn is_gff_resource_extension(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "gff"
            | "are"
            | "bic"
            | "dlg"
            | "fac"
            | "gic"
            | "git"
            | "gui"
            | "ifo"
            | "itp"
            | "jrl"
            | "utc"
            | "utd"
            | "ute"
            | "uti"
            | "utm"
            | "utp"
            | "uts"
            | "utt"
            | "utw"
    )
}

fn cached_scene(
    state: &ViewerServiceState,
    request: &ViewerAssetRequest,
) -> napi::Result<Arc<SceneDocument>> {
    let session = {
        let mut sessions = state.sessions.lock().map_err(|error| {
            napi::Error::from_reason(format!("viewer session map is poisoned: {error}"))
        })?;
        Arc::clone(sessions.get(&request.session_key).ok_or_else(|| {
            napi::Error::from_reason("viewer session is no longer available; reload the scene")
        })?)
    };
    let mut session = session.lock().map_err(|error| {
        napi::Error::from_reason(format!("viewer package session is poisoned: {error}"))
    })?;
    let session = session
        .as_mut()
        .ok_or_else(|| napi::Error::from_reason("viewer session was not initialized"))?;
    session
        .scenes
        .get(&request.asset_key)
        .map(|cached| Arc::clone(&cached.scene))
        .ok_or_else(|| {
            napi::Error::from_reason("viewer scene assets were evicted; reload the scene")
        })
}

fn package_session(
    state: &ViewerServiceState,
    request: &ViewerLoadRequest,
) -> napi::Result<Arc<Mutex<Option<ViewerSession>>>> {
    let mut sessions = state.sessions.lock().map_err(|error| {
        napi::Error::from_reason(format!("viewer session map is poisoned: {error}"))
    })?;
    if let Some(session) = sessions.get(&request.session_key) {
        return Ok(Arc::clone(session));
    }
    sessions.insert(request.session_key.clone(), Arc::new(Mutex::new(None)));
    sessions
        .get(&request.session_key)
        .map(Arc::clone)
        .ok_or_else(|| napi::Error::from_reason("viewer session cache rejected a new session"))
}

fn ensure_session(
    session: &mut Option<ViewerSession>,
    request: &ViewerLoadRequest,
) -> napi::Result<()> {
    let configuration_key = serde_json::to_string(&request.configuration())
        .map_err(|error| napi::Error::from_reason(error.to_string()))?;
    if session
        .as_ref()
        .is_none_or(|session| session.configuration_key != configuration_key)
    {
        *session =
            Some(build_session(request, configuration_key).map_err(napi::Error::from_reason)?);
    }
    Ok(())
}

fn scene_cache_key(
    request: &ViewerLoadRequest,
    resource: &ResolvedResRef,
    overlay_sources: &[(String, Vec<u8>)],
) -> String {
    let mut hasher = DefaultHasher::new();
    resource.to_string().hash(&mut hasher);
    request.area.hash(&mut hasher);
    request.path.hash(&mut hasher);
    if !overlay_sources.is_empty() {
        for (filename, contents) in overlay_sources {
            filename.hash(&mut hasher);
            contents.hash(&mut hasher);
        }
    } else if let Ok(metadata) = request.path.metadata() {
        metadata.len().hash(&mut hasher);
        metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .hash(&mut hasher);
    }
    format!("{resource}:{}", hasher.finish())
}

fn authored_area_overlay_sources(
    authored: &ViewerAuthoredAreaRequest,
) -> napi::Result<Vec<(String, Vec<u8>)>> {
    let mut sources = Vec::with_capacity(if authored.gic.is_some() { 3 } else { 2 });
    for (kind, path) in [
        ("ARE", Some(&authored.are)),
        ("GIT", Some(&authored.git)),
        ("GIC", authored.gic.as_ref()),
    ] {
        let Some(path) = path else { continue };
        let bytes = encode_gff_source(path, kind).map_err(napi::Error::from_reason)?;
        sources.push((
            format!("{}.{}", authored.resref, kind.to_ascii_lowercase()),
            bytes,
        ));
    }
    Ok(sources)
}

fn encode_gff_source(path: &Path, expected_type: &str) -> Result<Vec<u8>, String> {
    let source =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let json_source = path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.to_ascii_lowercase().ends_with(".json"));
    let root = if json_source {
        gff_root_from_json_bytes(&source)
    } else {
        read_gff_root(&mut Cursor::new(&source))
    }
    .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    let expected_file_type = format!("{:<4}", expected_type.to_ascii_uppercase());
    if root.file_type != expected_file_type {
        return Err(format!(
            "{} contains {} data, expected {expected_file_type}",
            path.display(),
            root.file_type
        ));
    }
    if !json_source {
        return Ok(source);
    }
    let mut output = Cursor::new(Vec::new());
    write_gff_root(&mut output, &root)
        .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
    Ok(output.into_inner())
}

fn project_resource_directories(project_root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    let mut visited = BTreeSet::new();
    collect_project_resource_directories(project_root, &mut visited, &mut result)?;
    Ok(result)
}

fn collect_project_resource_directories(
    project_root: &Path,
    visited: &mut BTreeSet<PathBuf>,
    result: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let normalized = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    if !visited.insert(normalized.clone()) {
        return Ok(());
    }
    let Some(manifest) = read_project_manifest(&normalized)? else {
        push_unique_path(result, normalized);
        return Ok(());
    };
    for dependency in manifest.dependencies.values() {
        let DependencySpec::Path(dependency) = dependency;
        let root = normalized.join(&dependency.path);
        collect_project_resource_directories(&root, visited, result)?;
    }
    push_unique_path(result, normalized.join(manifest.source.path));
    Ok(())
}

fn push_unique_path(target: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !target.iter().any(|existing| existing == &path) {
        target.push(path);
    }
}

fn default_language() -> String {
    "english".into()
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_localization() -> ViewerLocalization {
        ViewerLocalization {
            language:  Language::English,
            user_root: PathBuf::new(),
            dialog:    ViewerTlkLayer {
                male:            None,
                female:          None,
                male_resource:   None,
                female_resource: None,
            },
            custom:    HashMap::new(),
        }
    }

    #[test]
    fn localized_inspection_prefers_configured_inline_text() {
        let mut localization = empty_localization();
        let resolved = localization.resolve(
            &nwnrs_types::gff::GffCExoLocString {
                str_ref: 42,
                entries: vec![(0, "English".into()), (2, "German".into())],
            },
            None,
        );
        assert_eq!(resolved.text.as_deref(), Some("English"));
        assert_eq!(resolved.source.as_deref(), Some("inline"));
        assert_eq!(resolved.str_ref, Some(42));
    }

    #[test]
    fn localized_inspection_applies_the_custom_tlk_offset() {
        let mut localization = empty_localization();
        let mut custom = SingleTlk::new();
        custom.set_text(7, "Custom text");
        localization.custom.insert(
            "module_text".into(),
            ViewerTlkLayer {
                male:            Some(custom),
                female:          None,
                male_resource:   Some("module_text.tlk".into()),
                female_resource: None,
            },
        );
        let resolved = localization.resolve(
            &nwnrs_types::gff::GffCExoLocString {
                str_ref: 0x0100_0007,
                entries: Vec::new(),
            },
            Some("module_text"),
        );
        assert_eq!(resolved.text.as_deref(), Some("Custom text"));
        assert_eq!(resolved.source.as_deref(), Some("module_text.tlk"));
        assert_eq!(
            valid_custom_tlk_name("MODULE_TEXT.TLK"),
            Some("MODULE_TEXT")
        );
        assert_eq!(valid_custom_tlk_name("../escape"), None);
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nwnrs-vscode-viewer-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&path).expect("create temporary directory");
        path
    }

    fn entry(resource: String) -> ViewerResourceEntry {
        ViewerResourceEntry {
            resource,
            extension: "mdl".into(),
            family: "Models".into(),
            layer: "Vanilla".into(),
            origin: "KeyTable:test.key(test.bif)".into(),
            file_path: None,
        }
    }

    #[test]
    fn authored_area_sources_are_validated_and_encoded_in_memory() {
        let root = temporary_directory("authored-area");
        let are = root.join("start.are.json");
        let git = root.join("start.git.json");
        fs::write(&are, r#"{"__data_type":"ARE "}"#).expect("write ARE JSON");
        fs::write(&git, r#"{"__data_type":"GIT "}"#).expect("write GIT JSON");
        let sources = authored_area_overlay_sources(&ViewerAuthoredAreaRequest {
            resref: "start".into(),
            are,
            git,
            gic: None,
        })
        .expect("encode authored area");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].0, "start.are");
        assert_eq!(sources[1].0, "start.git");
        assert_eq!(
            read_gff_root(&mut Cursor::new(&sources[0].1))
                .expect("decode ARE")
                .file_type,
            "ARE "
        );
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn source_json_resources_use_compound_names_and_override_binary_build_outputs() {
        let root = temporary_directory("source-json");
        fs::write(root.join("creature.utc.json"), r#"{"__data_type":"UTC "}"#)
            .expect("write UTC JSON");
        let resources = gff_json_resources(&root).expect("collect GFF JSON");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].0, "creature.utc");
        fs::write(root.join("creature.utc"), &resources[0].1).expect("write binary UTC");
        let with_binary = gff_json_resources(&root).expect("prefer authored JSON");
        assert_eq!(with_binary.len(), 1);
        assert_eq!(with_binary[0].0, "creature.utc");
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn resource_families_cover_supported_editor_types() {
        assert_eq!(resource_family("mdl"), "Models");
        assert_eq!(resource_family("dds"), "Textures");
        assert_eq!(resource_family("nss"), "Scripts");
        assert_eq!(resource_family("utc"), "Blueprints");
        assert_eq!(resource_family("2da"), "Tables");
        assert_eq!(resource_family("are"), "Areas & Modules");
        assert_eq!(resource_family("dlg"), "Dialogs & UI");
        assert_eq!(resource_family("hak"), "Archives");
    }

    #[test]
    fn large_name_catalogs_are_partitioned_without_losing_parent_filters() {
        let resources = (0..201)
            .map(|index| entry(format!("c_{index:03}.mdl")))
            .collect::<Vec<_>>();
        let references = resources.iter().collect::<Vec<_>>();
        let root = name_catalog(&references, "");
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].kind, "prefix");
        assert_eq!(root[0].prefix.as_deref(), Some("c"));
        assert_eq!(root[0].count, 201);

        let leaves = name_catalog(&references, "c_000");
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].resource.as_deref(), Some("c_000.mdl"));
        assert_eq!(leaves[0].layer.as_deref(), Some("Vanilla"));
        assert_eq!(leaves[0].family.as_deref(), Some("Models"));
        assert_eq!(leaves[0].extension.as_deref(), Some("mdl"));
    }

    #[test]
    fn package_source_uses_ifo_areas_and_preserves_authored_paths() {
        let root = temporary_directory("package-source");
        fs::create_dir_all(root.join("areas")).expect("create areas directory");
        fs::create_dir_all(root.join("dialogs")).expect("create dialogs directory");
        fs::create_dir_all(root.join("code/shared")).expect("create code directory");
        fs::write(
            root.join(PROJECT_MANIFEST_FILENAME),
            "[project]\nname = \"source-test\"\nkind = \"mod\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write manifest");
        fs::write(
            root.join("module.ifo.json"),
            r#"{
  "__data_type": "IFO ",
  "Mod_Area_list": {
    "type": "list",
    "value": [
      { "__struct_id": 6, "Area_Name": { "type": "resref", "value": "start" } },
      { "__struct_id": 6, "Area_Name": { "type": "resref", "value": "missing" } }
    ]
  }
}"#,
        )
        .expect("write module IFO");
        fs::write(root.join("areas/start.are.json"), "{}").expect("write area");
        let mut git = nwnrs_types::gff::GffRoot::new("GIT ");
        let mut placeable = nwnrs_types::gff::GffStruct::new(9);
        placeable
            .put_value(
                "Tag",
                nwnrs_types::gff::GffValue::CExoString("test_chest".into()),
            )
            .expect("write placeable tag");
        placeable
            .put_value(
                "TemplateResRef",
                nwnrs_types::gff::GffValue::ResRef("plc_chest1".into()),
            )
            .expect("write placeable template");
        placeable
            .put_value("X", nwnrs_types::gff::GffValue::Float(4.0))
            .expect("write placeable x");
        placeable
            .put_value("Y", nwnrs_types::gff::GffValue::Float(5.0))
            .expect("write placeable y");
        git.put_value(
            "Placeable List",
            nwnrs_types::gff::GffValue::List(vec![placeable]),
        )
        .expect("write placeable list");
        let mut git_file = fs::File::create(root.join("areas/start.git")).expect("create git");
        write_gff_root(&mut git_file, &git).expect("write git");
        fs::write(root.join("areas/orphan.gic.json"), "{}").expect("write orphan gic");
        fs::write(root.join("dialogs/intro.dlg.json"), "{}").expect("write dialog");
        fs::write(root.join("code/main.nss"), "void main() {}\n").expect("write script");
        fs::write(root.join("code/shared/types.nss"), "// include\n").expect("write include");
        fs::write(root.join("nwscript.nss"), "// langspec\n").expect("write langspec");

        let result = inspect_package_source(&root.join(PROJECT_MANIFEST_FILENAME))
            .expect("inspect package source");
        assert_eq!(result.areas.len(), 3);
        assert_eq!(result.areas[0].resref, "start");
        assert!(result.areas[0].registered);
        assert_eq!(result.areas[0].missing, vec!["GIC"]);
        assert_eq!(result.areas[0].objects.len(), 1);
        assert_eq!(
            result.areas[0].objects[0].kind,
            nwnrs_types::scene::SceneInstanceKind::Placeable
        );
        assert_eq!(result.areas[0].objects[0].label, "test_chest");
        assert_eq!(result.areas[0].objects[0].position, [4.0, 5.0, 0.0]);
        assert!(result.areas[0].object_error.is_none());
        assert_eq!(result.areas[1].resref, "missing");
        assert!(result.areas[1].registered);
        assert_eq!(result.areas[1].missing, vec!["ARE", "GIT", "GIC"]);
        assert_eq!(result.areas[2].resref, "orphan");
        assert!(!result.areas[2].registered);
        assert_eq!(result.dialogs[0].relative_path, "dialogs/intro.dlg.json");
        assert_eq!(result.dialogs[0].kind, "dlgJson");
        assert_eq!(
            result
                .code
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["code/main.nss", "code/shared/types.nss"]
        );
        assert!(
            result
                .code
                .iter()
                .all(|file| file.relative_path != "nwscript.nss")
        );

        fs::remove_dir_all(root).expect("remove temporary directory");
    }
}
