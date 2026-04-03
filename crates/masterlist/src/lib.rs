#![forbid(unsafe_code)]
//! Async client types for the Beamdog NWN masterlist API.
//!
//! This crate models the JSON payloads returned by the public masterlist service and provides
//! a few direct fetch helpers. It is intentionally thin and keeps the response schema close to
//! the wire format.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument};

/// The Beamdog masterlist API base URL.
pub const URL: &str = "https://api.nwn.beamdog.net/v1";

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        Manifest, Me, Nwsync, Server, URL, get_my_servers, get_servers, get_servers_by_ip_and_port,
        get_servers_by_public_key,
    };
}

/// A single required or optional NWSync manifest entry.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Whether the manifest is required for connecting.
    pub required: bool,
    /// The manifest content hash.
    pub hash: String,
}

/// NWSync metadata advertised by a masterlist server entry.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nwsync {
    /// The manifests associated with the server.
    pub manifests: Vec<Manifest>,
    /// The base URL for the NWSync repository.
    pub url: String,
}

/// A single Beamdog masterlist server entry.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// The first time the server was seen by the masterlist.
    #[serde(rename = "first_seen")]
    pub first_seen: i64,
    /// The most recent advertisement timestamp.
    #[serde(rename = "last_advertisement")]
    pub last_advertisement: i64,
    /// The advertised session name.
    #[serde(rename = "session_name")]
    pub session_name: String,
    /// The advertised module name.
    #[serde(rename = "module_name")]
    pub module_name: String,
    /// The advertised module description.
    #[serde(rename = "module_description")]
    pub module_description: String,
    /// Whether the server is password protected.
    pub passworded: bool,
    /// The minimum character level.
    #[serde(rename = "min_level")]
    pub min_level: i64,
    /// The maximum character level.
    #[serde(rename = "max_level")]
    pub max_level: i64,
    /// The current player count.
    #[serde(rename = "current_players")]
    pub current_players: i64,
    /// The maximum supported player count.
    #[serde(rename = "max_players")]
    pub max_players: i64,
    /// The advertised build string.
    pub build: String,
    /// The advertised revision number.
    pub rev: i64,
    /// The PVP mode identifier.
    pub pvp: i64,
    /// Whether the server uses a server vault.
    pub servervault: bool,
    /// Whether enforce legal characters is enabled.
    pub elc: bool,
    /// Whether item level restrictions are enabled.
    pub ilr: bool,
    /// Whether the server is configured for one party.
    #[serde(rename = "one_party")]
    pub one_party: bool,
    /// Whether players can pause the game.
    #[serde(rename = "player_pause")]
    pub player_pause: bool,
    /// The operating system identifier.
    pub os: i64,
    /// The language identifier.
    pub language: i64,
    /// The game type identifier.
    #[serde(rename = "game_type")]
    pub game_type: i64,
    /// The measured latency.
    pub latency: i64,
    /// The host or IP address.
    pub host: String,
    /// The host port.
    pub port: i64,
    /// The advertised key-exchange public key, if present.
    #[serde(rename = "kx_pk")]
    pub kx_pk: Option<String>,
    /// The advertised signing public key, if present.
    #[serde(rename = "sign_pk")]
    pub sign_pk: Option<String>,
    /// The advertised NWSync details, if present.
    pub nwsync: Option<Nwsync>,
    /// An optional connection hint.
    pub connecthint: Option<String>,
}

/// The `/me` response payload.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    /// The requester address as seen by the masterlist.
    pub address: String,
    /// The server entries associated with that address.
    pub servers: Vec<Server>,
}

#[instrument(level = "debug", skip_all, err, fields(url = %url))]
async fn get_json<T>(url: String) -> Result<T, reqwest::Error>
where
    T: DeserializeOwned,
{
    debug!("fetching masterlist json");
    reqwest::get(url).await?.json::<T>().await
}

/// Fetches the `/me` response for the current caller.
#[instrument(level = "info", err)]
pub async fn get_my_servers() -> Result<Me, reqwest::Error> {
    info!("fetching current caller masterlist servers");
    get_json(format!("{URL}/me")).await
}

/// Fetches the full advertised server list.
#[instrument(level = "info", err)]
pub async fn get_servers() -> Result<Vec<Server>, reqwest::Error> {
    info!("fetching full masterlist server list");
    get_json(format!("{URL}/servers")).await
}

/// Fetches all servers advertising the given public key.
#[instrument(level = "info", skip_all, err, fields(public_key = %public_key))]
pub async fn get_servers_by_public_key(public_key: String) -> Result<Vec<Server>, reqwest::Error> {
    info!("fetching masterlist servers by public key");
    get_json(format!("{URL}/servers/{public_key}")).await
}

/// Fetches all servers matching the given IP address and port.
#[instrument(level = "info", skip_all, err, fields(ip = %ip, port))]
pub async fn get_servers_by_ip_and_port(
    ip: String,
    port: i32,
) -> Result<Vec<Server>, reqwest::Error> {
    info!("fetching masterlist servers by address");
    get_json(format!("{URL}/servers/{ip}/{port}")).await
}
