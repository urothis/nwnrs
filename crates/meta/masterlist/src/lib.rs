#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use serde::{Deserialize, Serialize, de::DeserializeOwned};
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

/// A single required or optional `NWSync` manifest entry.
///
/// # Examples
///
/// ```rust,no_run
/// let manifest = nwnrs_masterlist::Manifest::default();
/// assert!(!manifest.required);
/// ```
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Whether the manifest is required for connecting.
    pub required: bool,
    /// The manifest content hash.
    pub hash:     String,
}

/// `NWSync` metadata advertised by a masterlist server entry.
///
/// # Examples
///
/// ```rust,no_run
/// let nwsync = nwnrs_masterlist::Nwsync::default();
/// assert!(nwsync.manifests.is_empty());
/// ```
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nwsync {
    /// The manifests associated with the server.
    pub manifests: Vec<Manifest>,
    /// The base URL for the `NWSync` repository.
    pub url:       String,
}

/// A single Beamdog masterlist server entry.
///
/// # Examples
///
/// ```rust,no_run
/// let server = nwnrs_masterlist::Server::default();
/// assert_eq!(server.current_players, 0);
/// ```
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct Server {
    /// The first time the server was seen by the masterlist.
    #[serde(rename = "first_seen")]
    pub first_seen:         i64,
    /// The most recent advertisement timestamp.
    #[serde(rename = "last_advertisement")]
    pub last_advertisement: i64,
    /// The advertised session name.
    #[serde(rename = "session_name")]
    pub session_name:       String,
    /// The advertised module name.
    #[serde(rename = "module_name")]
    pub module_name:        String,
    /// The advertised module description.
    #[serde(rename = "module_description")]
    pub module_description: String,
    /// Whether the server is password protected.
    pub passworded:         bool,
    /// The minimum character level.
    #[serde(rename = "min_level")]
    pub min_level:          i64,
    /// The maximum character level.
    #[serde(rename = "max_level")]
    pub max_level:          i64,
    /// The current player count.
    #[serde(rename = "current_players")]
    pub current_players:    i64,
    /// The maximum supported player count.
    #[serde(rename = "max_players")]
    pub max_players:        i64,
    /// The advertised build string.
    pub build:              String,
    /// The advertised revision number.
    pub rev:                i64,
    /// The PVP mode identifier.
    pub pvp:                i64,
    /// Whether the server uses a server vault.
    pub servervault:        bool,
    /// Whether enforce legal characters is enabled.
    pub elc:                bool,
    /// Whether item level restrictions are enabled.
    pub ilr:                bool,
    /// Whether the server is configured for one party.
    #[serde(rename = "one_party")]
    pub one_party:          bool,
    /// Whether players can pause the game.
    #[serde(rename = "player_pause")]
    pub player_pause:       bool,
    /// The operating system identifier.
    pub os:                 i64,
    /// The language identifier.
    pub language:           i64,
    /// The game type identifier.
    #[serde(rename = "game_type")]
    pub game_type:          i64,
    /// The measured latency.
    pub latency:            i64,
    /// The host or IP address.
    pub host:               String,
    /// The host port.
    pub port:               i64,
    /// The advertised key-exchange public key, if present.
    #[serde(rename = "kx_pk")]
    pub kx_pk:              Option<String>,
    /// The advertised signing public key, if present.
    #[serde(rename = "sign_pk")]
    pub sign_pk:            Option<String>,
    /// The advertised `NWSync` details, if present.
    pub nwsync:             Option<Nwsync>,
    /// An optional connection hint.
    pub connecthint:        Option<String>,
}

/// The `/me` response payload.
///
/// # Examples
///
/// ```rust,no_run
/// let me = nwnrs_masterlist::Me::default();
/// assert!(me.servers.is_empty());
/// ```
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
///
/// # Errors
///
/// Returns [`reqwest::Error`] if the request fails or the response cannot be
/// deserialized.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_masterlist::get_my_servers;
/// ```
#[instrument(level = "info", err)]
pub async fn get_my_servers() -> Result<Me, reqwest::Error> {
    info!("fetching current caller masterlist servers");
    get_json(format!("{URL}/me")).await
}

/// Fetches the full advertised server list.
///
/// # Errors
///
/// Returns [`reqwest::Error`] if the request fails or the response cannot be
/// deserialized.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_masterlist::get_servers;
/// ```
#[instrument(level = "info", err)]
pub async fn get_servers() -> Result<Vec<Server>, reqwest::Error> {
    info!("fetching full masterlist server list");
    get_json(format!("{URL}/servers")).await
}

/// Fetches all servers advertising the given public key.
///
/// # Errors
///
/// Returns [`reqwest::Error`] if the request fails or the response cannot be
/// deserialized.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_masterlist::get_servers_by_public_key;
/// ```
#[instrument(level = "info", skip_all, err, fields(public_key = %public_key))]
pub async fn get_servers_by_public_key(public_key: String) -> Result<Vec<Server>, reqwest::Error> {
    info!("fetching masterlist servers by public key");
    get_json(format!("{URL}/servers/{public_key}")).await
}

/// Fetches all servers matching the given IP address and port.
///
/// # Errors
///
/// Returns [`reqwest::Error`] if the request fails or the response cannot be
/// deserialized.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_masterlist::get_servers_by_ip_and_port;
/// ```
#[instrument(level = "info", skip_all, err, fields(ip = %ip, port))]
pub async fn get_servers_by_ip_and_port(
    ip: String,
    port: i32,
) -> Result<Vec<Server>, reqwest::Error> {
    info!("fetching masterlist servers by address");
    get_json(format!("{URL}/servers/{ip}/{port}")).await
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Server;

    #[test]
    fn deserializes_masterlist_server_wire_shape() {
        let value = json!({
            "first_seen": 1,
            "last_advertisement": 2,
            "session_name": "Test Server",
            "module_name": "Module",
            "module_description": "Desc",
            "passworded": false,
            "min_level": 1,
            "max_level": 40,
            "current_players": 3,
            "max_players": 64,
            "build": "8193.37",
            "rev": 42,
            "pvp": 1,
            "servervault": true,
            "elc": false,
            "ilr": false,
            "one_party": true,
            "player_pause": false,
            "os": 2,
            "language": 0,
            "game_type": 5,
            "latency": 12,
            "host": "127.0.0.1",
            "port": 5121,
            "kx_pk": "kx",
            "sign_pk": "sign",
            "connecthint": "hint",
            "nwsync": {
                "url": "https://example.com/nwsync",
                "manifests": [
                    { "required": true, "hash": "abc" }
                ]
            }
        });

        let server: Server = match serde_json::from_value(value) {
            Ok(value) => value,
            Err(error) => panic!("deserialize server: {error}"),
        };
        assert_eq!(server.session_name, "Test Server");
        assert_eq!(server.nwsync.as_ref().map(|n| n.manifests.len()), Some(1));
        assert_eq!(server.kx_pk.as_deref(), Some("kx"));
    }
}
