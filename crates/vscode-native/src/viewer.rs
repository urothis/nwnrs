use std::{
    collections::BTreeSet,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::UNIX_EPOCH,
};

use napi::{
    Task,
    bindgen_prelude::{AsyncTask, Buffer},
};
use napi_derive::napi;
use nwnrs_nwpkg::{DependencySpec, read_project_manifest};
use nwnrs_renderer::{RenderScene, SceneLoader, ScenePacket};
use nwnrs_types::{
    gff::{ARE_RES_TYPE, GIT_RES_TYPE, IFO_RES_TYPE},
    install::{find_nwnrs_root, find_user_root, new_default_resman},
    lru::WeightedLru,
    mdl::{ModelResourceKind, NwnBlueprintKind},
    resman::{ResContainer, ResMan, ResolvedResRef, read_resmemfile},
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
}

struct CachedViewerScene {
    scene:   Arc<RenderScene>,
    catalog: Arc<Vec<u8>>,
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

/// Background dependency provenance task.
pub struct ViewerResolveTask {
    state:        Arc<ViewerServiceState>,
    request_json: String,
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
        let cache_key = scene_cache_key(&request, &resource, self.contents.as_deref());
        if let Some(cached) = session.scenes.get(&cache_key) {
            return Ok(cached.catalog.as_ref().clone());
        }
        let overlay = if let Some(contents) = self.contents.take() {
            let memory =
                read_resmemfile(filename, resource.clone().into(), contents).map_err(|error| {
                    napi::Error::from_reason(format!("virtual viewer resource: {error}"))
                })?;
            let overlay = Arc::new(memory) as Arc<dyn ResContainer>;
            session.resman.add(Arc::clone(&overlay));
            Some(overlay)
        } else {
            None
        };
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
                Err(nwnrs_renderer::RendererError::invalid(format!(
                    "{} does not have a 3D scene provider",
                    resource
                )))
            }
            .map_err(|error| napi::Error::from_reason(error.to_string()))
        };
        if let Some(overlay) = overlay {
            session.resman.remove(&overlay);
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

#[derive(Debug, Clone, Deserialize)]
struct ViewerLoadRequest {
    session_key:  String,
    path:         PathBuf,
    project_root: PathBuf,
    #[serde(default)]
    area:         Option<String>,
    #[serde(default)]
    root:         Option<PathBuf>,
    #[serde(default)]
    user:         Option<PathBuf>,
    #[serde(default = "default_language")]
    language:     String,
    #[serde(default)]
    load_ovr:     bool,
    #[serde(default)]
    archives:     Vec<PathBuf>,
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

#[derive(Serialize)]
struct ViewerConfiguration<'a> {
    project_root: &'a Path,
    root:         &'a Option<PathBuf>,
    user:         &'a Option<PathBuf>,
    language:     &'a str,
    load_ovr:     bool,
    resource_dir: Option<&'a Path>,
    archives:     &'a [PathBuf],
}

impl ViewerLoadRequest {
    fn configuration(&self) -> ViewerConfiguration<'_> {
        ViewerConfiguration {
            project_root: &self.project_root,
            root:         &self.root,
            user:         &self.user,
            language:     &self.language,
            load_ovr:     self.load_ovr,
            resource_dir: self.path.parent(),
            archives:     &self.archives,
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
    let mut directories = project_resource_directories(&request.project_root)?;
    if let Some(parent) = request.path.parent() {
        push_unique_path(&mut directories, parent.to_path_buf());
    }
    let resman = new_default_resman(
        root,
        user,
        &request.language,
        96,
        true,
        request.load_ovr,
        &[],
        &request.archives,
        &directories,
        &[],
    )
    .map_err(|error| error.to_string())?;
    Ok(ViewerSession {
        configuration_key,
        resman,
        scenes: WeightedLru::new(96 * 1024 * 1024, 1),
    })
}

fn cached_scene(
    state: &ViewerServiceState,
    request: &ViewerAssetRequest,
) -> napi::Result<Arc<RenderScene>> {
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
    contents: Option<&[u8]>,
) -> String {
    let mut hasher = DefaultHasher::new();
    resource.to_string().hash(&mut hasher);
    request.area.hash(&mut hasher);
    request.path.hash(&mut hasher);
    if let Some(contents) = contents {
        contents.hash(&mut hasher);
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
