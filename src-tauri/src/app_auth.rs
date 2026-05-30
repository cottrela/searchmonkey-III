use crate::plugins::runtime::{InstalledPluginInfo, PluginIndexRuntime, PluginIndexSummary};
use anyhow::{anyhow, Context, Result};
use keyring::Entry;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const APP_AUTH_START_URL: &str = "https://searchmonkey.dev/api/app-auth/start";
const APP_AUTH_POLL_URL: &str = "https://searchmonkey.dev/api/app-auth/poll";
const APP_ENTITLEMENTS_URL: &str = "https://searchmonkey.dev/api/app/entitlements";
const KEYRING_SERVICE: &str = "uk.co.axonara.searchmonkey.app-auth";
const KEYRING_ACCOUNT: &str = "session-token";
const APP_AUTH_EVENT: &str = "app-auth-updated";
const LINUX_PLAIN_TOKEN_WARNING: &str =
    "Secure storage is unavailable on this Linux system. The purchase session is stored in plain text.";

#[derive(Debug, Clone, Serialize)]
pub struct PurchaseConnectionSummary {
    pub state: String,
    pub email: Option<String>,
    pub pending_email: Option<String>,
    pub pending_expires_at: Option<String>,
    pub last_synced_at: Option<String>,
    pub has_cached_entitlements: bool,
    pub status_message: Option<String>,
    pub storage_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketplacePluginSummary {
    pub plugin_id: String,
    pub name: String,
    pub owned: bool,
    pub latest_version: Option<String>,
    pub download_url: Option<String>,
    pub buy_url: Option<String>,
    pub homepage_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum ConnectionState {
    #[default]
    NotConnected,
    Pending,
    Connected,
    Expired,
}

impl ConnectionState {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotConnected => "not_connected",
            Self::Pending => "pending",
            Self::Connected => "connected",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedAppAuthState {
    #[serde(default)]
    connection_state: ConnectionState,
    email: Option<String>,
    #[serde(default)]
    has_session_token: bool,
    pending_email: Option<String>,
    pending_request_id: Option<String>,
    pending_expires_at: Option<String>,
    last_synced_at: Option<String>,
    last_error: Option<String>,
    storage_warning: Option<String>,
    #[serde(default)]
    entitlements: Vec<CachedEntitlement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedEntitlement {
    plugin_id: String,
    name: Option<String>,
    owned: bool,
    latest_version: Option<String>,
    download_url: Option<String>,
    buy_url: Option<String>,
    homepage_url: Option<String>,
}

#[derive(Debug, Clone)]
struct StartResponse {
    request_id: String,
    expires_at: String,
}

#[derive(Debug, Clone)]
enum PollStatus {
    Pending,
    Approved {
        session_token: String,
        email: Option<String>,
        entitlements: Vec<CachedEntitlement>,
    },
}

#[derive(Clone)]
pub struct AppAuthRuntime {
    state_path: PathBuf,
    linux_plaintext_token_path: PathBuf,
}

impl Default for AppAuthRuntime {
    fn default() -> Self {
        let base_dir = default_state_dir();
        Self {
            state_path: base_dir.join("app-auth.json"),
            linux_plaintext_token_path: base_dir.join("app-auth-session-token.txt"),
        }
    }
}

impl AppAuthRuntime {
    pub fn purchase_summary(
        &self,
        installed_plugins: &[InstalledPluginInfo],
    ) -> (PurchaseConnectionSummary, Vec<MarketplacePluginSummary>) {
        let state = self.load_state().unwrap_or_default();
        let effective_state =
            if state.connection_state == ConnectionState::Connected && !state.has_session_token {
                ConnectionState::Expired
            } else {
                state.connection_state
            };
        let effective_status_message = if state.connection_state == ConnectionState::Connected
            && !state.has_session_token
        {
            Some("Reconnect purchases to restore the secure session for refresh and install actions.".to_string())
        } else {
            state.last_error.clone()
        };
        let installed_names = installed_plugins
            .iter()
            .map(|plugin| (plugin.id.clone(), plugin.name.clone()))
            .collect::<HashMap<_, _>>();
        let marketplace_plugins = state
            .entitlements
            .iter()
            .map(|plugin| MarketplacePluginSummary {
                plugin_id: plugin.plugin_id.clone(),
                name: plugin
                    .name
                    .clone()
                    .or_else(|| installed_names.get(&plugin.plugin_id).cloned())
                    .unwrap_or_else(|| plugin.plugin_id.clone()),
                owned: plugin.owned,
                latest_version: plugin.latest_version.clone(),
                download_url: plugin.download_url.clone(),
                buy_url: plugin.buy_url.clone(),
                homepage_url: plugin.homepage_url.clone(),
            })
            .collect::<Vec<_>>();

        (
            PurchaseConnectionSummary {
                state: effective_state.as_str().to_string(),
                email: state.email,
                pending_email: state.pending_email,
                pending_expires_at: state.pending_expires_at,
                last_synced_at: state.last_synced_at,
                has_cached_entitlements: !state.entitlements.is_empty(),
                status_message: effective_status_message,
                storage_warning: state.storage_warning,
            },
            marketplace_plugins,
        )
    }

    pub fn start_email_verification(&self, app: &AppHandle, email: &str) -> Result<()> {
        let email = email.trim();
        if email.is_empty() {
            anyhow::bail!("Email is required.");
        }

        let response = start_app_auth_request(email, app)?;
        let mut state = self.load_state().unwrap_or_default();
        state.connection_state = ConnectionState::Pending;
        state.has_session_token = false;
        state.pending_email = Some(email.to_string());
        state.pending_request_id = Some(response.request_id);
        state.pending_expires_at = Some(response.expires_at);
        state.last_error = Some("Check your email to approve this device.".to_string());
        self.write_state(&state)?;
        self.emit_updated(app);
        Ok(())
    }

    pub fn poll_pending_request(&self, app: &AppHandle) -> Result<bool> {
        let state = self.load_state()?;
        let request_id = state
            .pending_request_id
            .as_deref()
            .ok_or_else(|| anyhow!("No purchase connection is waiting for verification."))?;

        match poll_app_auth_request(request_id)? {
            PollStatus::Pending => {
                let mut state = state;
                state.connection_state = ConnectionState::Pending;
                state.last_error = Some("Waiting for email verification...".to_string());
                self.write_state(&state)?;
                self.emit_updated(app);
                Ok(false)
            }
            PollStatus::Approved {
                session_token,
                email,
                entitlements,
            } => {
                self.write_session_token(&session_token)?;
                self.persist_connected_state(email.or(state.pending_email), entitlements)?;
                self.emit_updated(app);
                Ok(true)
            }
        }
    }

    pub fn refresh_entitlements(&self, app: &AppHandle) -> Result<()> {
        let token = self
            .read_session_token()?
            .ok_or_else(|| anyhow!("Connect purchases before refreshing."))?;
        let response = fetch_entitlements(&token)?;
        self.persist_connected_state(response.email, response.entitlements)?;
        self.emit_updated(app);
        Ok(())
    }

    pub fn disconnect(&self, app: &AppHandle) -> Result<()> {
        let mut state = self.load_state().unwrap_or_default();
        state.connection_state = ConnectionState::NotConnected;
        state.email = None;
        state.has_session_token = false;
        state.pending_email = None;
        state.pending_request_id = None;
        state.pending_expires_at = None;
        state.last_synced_at = None;
        state.last_error = None;
        state.storage_warning = None;
        state.entitlements.clear();
        self.write_state(&state)?;
        self.delete_session_token();
        self.emit_updated(app);
        Ok(())
    }

    pub fn install_marketplace_plugin(
        &self,
        app: &AppHandle,
        plugin_index: &PluginIndexRuntime,
        plugin_id: &str,
    ) -> Result<(String, String, PluginIndexSummary)> {
        let state = self.load_state()?;
        let token = self
            .read_session_token()?
            .ok_or_else(|| anyhow!("Connect purchases before installing plugins."))?;
        let plugin = state
            .entitlements
            .iter()
            .find(|plugin| plugin.plugin_id == plugin_id)
            .cloned()
            .ok_or_else(|| anyhow!("Plugin {plugin_id} is not available in your purchases."))?;
        if !plugin.owned {
            anyhow::bail!("Plugin {plugin_id} has not been purchased.");
        }
        let download_url = plugin
            .download_url
            .as_deref()
            .ok_or_else(|| anyhow!("Plugin download is not available for {plugin_id}."))?;
        let archive_path = download_plugin_archive(download_url, &token, plugin_id)?;
        let install_result = plugin_index.install_plugin_archive(&archive_path);
        let _ = fs::remove_file(&archive_path);
        let install_result = install_result?;
        self.emit_updated(app);
        Ok(install_result)
    }

    pub fn emit_updated(&self, app: &AppHandle) {
        let _ = app.emit(APP_AUTH_EVENT, ());
    }

    fn persist_connected_state(
        &self,
        email: Option<String>,
        entitlements: Vec<CachedEntitlement>,
    ) -> Result<()> {
        let mut state = self.load_state().unwrap_or_default();
        state.connection_state = ConnectionState::Connected;
        state.email = email;
        state.pending_email = None;
        state.pending_request_id = None;
        state.pending_expires_at = None;
        state.last_synced_at = Some(now_rfc3339());
        state.last_error = None;
        state.has_session_token = true;
        state.storage_warning = if cfg!(target_os = "linux") && self.store_mode_is_plaintext()? {
            Some(LINUX_PLAIN_TOKEN_WARNING.to_string())
        } else {
            None
        };
        state.entitlements = entitlements;
        self.write_state(&state)
    }

    fn mark_expired(&self, message: &str) -> Result<()> {
        let mut state = self.load_state().unwrap_or_default();
        state.connection_state = ConnectionState::Expired;
        state.has_session_token = false;
        state.pending_email = None;
        state.pending_request_id = None;
        state.pending_expires_at = None;
        state.last_error = Some(message.to_string());
        self.write_state(&state)?;
        self.delete_session_token();
        Ok(())
    }

    fn load_state(&self) -> Result<PersistedAppAuthState> {
        if !self.state_path.exists() {
            return Ok(PersistedAppAuthState::default());
        }
        let contents = fs::read_to_string(&self.state_path)
            .with_context(|| format!("failed reading {}", self.state_path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("failed parsing {}", self.state_path.display()))
    }

    fn write_state(&self, state: &PersistedAppAuthState) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating {}", parent.display()))?;
        }
        fs::write(&self.state_path, serde_json::to_vec_pretty(state)?)
            .with_context(|| format!("failed writing {}", self.state_path.display()))
    }

    fn read_session_token(&self) -> Result<Option<String>> {
        match Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(_) => {
                if cfg!(target_os = "linux") {
                    return self.read_linux_plaintext_token();
                }
                Ok(None)
            }
        }
    }

    fn write_session_token(&self, token: &str) -> Result<()> {
        match Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)?.set_password(token) {
            Ok(()) => {
                if self.linux_plaintext_token_path.exists() {
                    let _ = fs::remove_file(&self.linux_plaintext_token_path);
                }
                Ok(())
            }
            Err(err) => {
                if cfg!(target_os = "linux") {
                    if let Some(parent) = self.linux_plaintext_token_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&self.linux_plaintext_token_path, token)?;
                    let mut state = self.load_state().unwrap_or_default();
                    state.has_session_token = true;
                    state.storage_warning = Some(LINUX_PLAIN_TOKEN_WARNING.to_string());
                    state.last_error = Some(format!(
                        "Secure storage is unavailable on Linux; using a plain-text fallback ({err})."
                    ));
                    self.write_state(&state)?;
                    Ok(())
                } else {
                    Err(err).context("failed storing purchase session token")
                }
            }
        }
    }

    fn delete_session_token(&self) {
        if let Ok(entry) = Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
            let _ = entry.delete_credential();
        }
        if self.linux_plaintext_token_path.exists() {
            let _ = fs::remove_file(&self.linux_plaintext_token_path);
        }
    }

    fn read_linux_plaintext_token(&self) -> Result<Option<String>> {
        if !self.linux_plaintext_token_path.exists() {
            return Ok(None);
        }
        let token = fs::read_to_string(&self.linux_plaintext_token_path).with_context(|| {
            format!(
                "failed reading {}",
                self.linux_plaintext_token_path.display()
            )
        })?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Ok(None);
        }
        Ok(Some(token))
    }

    fn store_mode_is_plaintext(&self) -> Result<bool> {
        Ok(cfg!(target_os = "linux") && self.read_linux_plaintext_token()?.is_some())
    }
}

#[derive(Debug)]
struct EntitlementsResponse {
    email: Option<String>,
    entitlements: Vec<CachedEntitlement>,
}

fn start_app_auth_request(email: &str, app: &AppHandle) -> Result<StartResponse> {
    let client = http_client()?;
    let payload = serde_json::json!({
        "email": email,
        "device_name": device_name(),
        "platform": current_platform_string(),
        "app_version": app.package_info().version.to_string()
    });
    let response = client
        .post(APP_AUTH_START_URL)
        .header(ACCEPT, "application/json")
        .json(&payload)
        .send()
        .context("failed contacting searchmonkey.dev")?;
    if !response.status().is_success() {
        anyhow::bail!("Purchase connection failed with {}", response.status());
    }
    let value = response.json::<Value>().context("invalid start response")?;
    let request_id = string_field(&value, &["request_id"])
        .ok_or_else(|| anyhow!("Start response did not include a request ID."))?;
    let expires_at = string_field(&value, &["expires_at"])
        .ok_or_else(|| anyhow!("Start response did not include an expiry time."))?;
    Ok(StartResponse {
        request_id,
        expires_at,
    })
}

fn poll_app_auth_request(request_id: &str) -> Result<PollStatus> {
    let client = http_client()?;
    let response = client
        .post(APP_AUTH_POLL_URL)
        .header(ACCEPT, "application/json")
        .json(&serde_json::json!({ "request_id": request_id }))
        .send()
        .context("failed polling searchmonkey.dev")?;
    if !response.status().is_success() {
        anyhow::bail!(
            "Purchase verification check failed with {}",
            response.status()
        );
    }
    let value = response.json::<Value>().context("invalid poll response")?;
    match string_field(&value, &["status"]).as_deref() {
        Some("pending") => Ok(PollStatus::Pending),
        Some("approved") => Ok(PollStatus::Approved {
            session_token: string_field(&value, &["session_token"])
                .ok_or_else(|| anyhow!("Approved response did not include a session token."))?,
            email: string_field(&value, &["email", "account_email", "user_email"]),
            entitlements: extract_entitlements(&value),
        }),
        Some(status) => anyhow::bail!("Unexpected purchase verification status: {status}"),
        None => anyhow::bail!("Poll response did not include a status."),
    }
}

fn fetch_entitlements(token: &str) -> Result<EntitlementsResponse> {
    let client = http_client()?;
    let response = client
        .get(APP_ENTITLEMENTS_URL)
        .header(ACCEPT, "application/json")
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .context("failed loading purchases from searchmonkey.dev")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!("Your purchase connection has expired.");
    }
    if !response.status().is_success() {
        anyhow::bail!("Purchase refresh failed with {}", response.status());
    }
    let value = response
        .json::<Value>()
        .context("invalid entitlements response")?;
    Ok(EntitlementsResponse {
        email: string_field(&value, &["email", "account_email", "user_email"]),
        entitlements: extract_entitlements(&value),
    })
}

fn extract_entitlements(value: &Value) -> Vec<CachedEntitlement> {
    value
        .get("entitlements")
        .or_else(|| value.get("plugins"))
        .or_else(|| value.get("items"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_entitlement_item)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_entitlement_item(value: &Value) -> Option<CachedEntitlement> {
    let plugin_id = string_field(value, &["plugin_id", "id", "slug"])?;
    let platform_metadata = current_platform_metadata(value);
    Some(CachedEntitlement {
        plugin_id,
        name: string_field(value, &["name", "title"]),
        owned: bool_field(value, &["owned", "purchased", "entitled", "active"]).unwrap_or(true),
        latest_version: string_field(value, &["latest_version", "version"])
            .or_else(|| platform_string_field(platform_metadata, &["version", "latest_version"])),
        download_url: string_field(
            value,
            &["download_url", "package_url", "install_url", "asset_url"],
        )
        .or_else(|| {
            platform_string_field(
                platform_metadata,
                &["download_url", "package_url", "install_url", "asset_url"],
            )
        }),
        buy_url: string_field(
            value,
            &["buy_url", "product_url", "checkout_url", "purchase_url"],
        )
        .or_else(|| deep_string_field(value, &["pricing", "purchase_url"])),
        homepage_url: string_field(value, &["homepage_url", "url", "homepage"]),
    })
}

fn current_platform_metadata<'a>(value: &'a Value) -> Option<&'a Value> {
    value
        .get("platforms")
        .and_then(Value::as_object)
        .and_then(|platforms| platforms.get(&current_platform_string()))
}

fn platform_string_field(value: Option<&Value>, keys: &[&str]) -> Option<String> {
    let value = value?;
    string_field(value, keys)
}

fn deep_string_field(value: &Value, path: &[&str]) -> Option<String> {
    let nested = path
        .iter()
        .try_fold(value, |current, key| current.get(key))?;
    nested
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn download_plugin_archive(download_url: &str, token: &str, plugin_id: &str) -> Result<PathBuf> {
    let client = http_client()?;
    let response = client
        .get(download_url)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .with_context(|| format!("failed downloading plugin package from {download_url}"))?;
    if !response.status().is_success() {
        anyhow::bail!("Plugin download failed with {}", response.status());
    }
    let bytes = response.bytes().context("failed reading plugin package")?;
    let archive_path = std::env::temp_dir().join(format!(
        "{plugin_id}-{}.smplugin",
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    fs::write(&archive_path, &bytes)
        .with_context(|| format!("failed writing {}", archive_path.display()))?;
    Ok(archive_path)
}

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(format!("Searchmonkey/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed creating HTTP client")
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(key))
        .and_then(Value::as_bool)
}

fn current_platform_string() -> String {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "windows-x64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        (os, arch) => return format!("{os}-{arch}"),
    }
    .to_string()
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn default_state_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/Searchmonkey-3");
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("Searchmonkey-3");
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(config_home).join("searchmonkey-3");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config/searchmonkey-3");
        }
    }

    Path::new(".").to_path_buf()
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Searchmonkey".to_string())
}

pub fn decorate_plugin_summary(
    plugin_index: &PluginIndexRuntime,
    app_auth: &AppAuthRuntime,
) -> PluginIndexSummary {
    let mut summary = plugin_index.summary();
    let (purchase_connection, marketplace_plugins) =
        app_auth.purchase_summary(&summary.installed_plugins);
    summary.purchase_connection = purchase_connection;
    summary.marketplace_plugins = marketplace_plugins;
    summary
}

pub fn reconcile_refresh_failure(runtime: &AppAuthRuntime, error: &anyhow::Error) {
    let message = error.to_string();
    if message.contains("expired")
        || message.contains("401")
        || message.contains("Connect purchases before")
    {
        let _ = runtime.mark_expired(&message);
        return;
    }
    if let Ok(mut state) = runtime.load_state() {
        state.last_error = Some(message);
        let _ = runtime.write_state(&state);
    }
}

#[cfg(test)]
mod tests {
    use super::{current_platform_string, extract_entitlements, parse_entitlement_item};
    use serde_json::json;

    #[test]
    fn parses_catalog_style_plugin_payload() {
        let current_platform = current_platform_string();
        let download_url =
            format!("https://searchmonkey.dev/api/plugins/sm.plugin.pdf/download?platform={current_platform}");
        let plugin = parse_entitlement_item(&json!({
            "id": "sm.plugin.pdf",
            "name": "PDF Extractor",
            "latest_version": "0.2.9",
            "homepage_url": "https://searchmonkey.dev/plugins/sm.plugin.pdf",
            "platforms": {
                current_platform: {
                    "version": "0.2.9",
                    "download_url": download_url
                }
            },
            "pricing": {
                "purchase_url": "https://buy.stripe.com/example"
            }
        }))
        .expect("plugin should parse");

        assert_eq!(plugin.plugin_id, "sm.plugin.pdf");
        assert_eq!(plugin.latest_version.as_deref(), Some("0.2.9"));
        assert_eq!(plugin.download_url.as_deref(), Some(download_url.as_str()));
        assert_eq!(
            plugin.buy_url.as_deref(),
            Some("https://buy.stripe.com/example")
        );
    }

    #[test]
    fn extracts_plugins_from_catalog_root() {
        let entitlements = extract_entitlements(&json!({
            "schema": "sm.plugin-catalog.v1",
            "plugins": [
                {
                    "id": "sm.plugin.ocr",
                    "name": "Image OCR",
                    "latest_version": "0.1.9",
                    "platforms": {
                        "linux-x64": {
                            "version": "0.1.9",
                            "download_url": "https://searchmonkey.dev/api/plugins/sm.plugin.ocr/download?platform=linux-x64"
                        }
                    }
                }
            ]
        }));

        assert_eq!(entitlements.len(), 1);
        assert_eq!(entitlements[0].plugin_id, "sm.plugin.ocr");
        assert_eq!(entitlements[0].latest_version.as_deref(), Some("0.1.9"));
    }
}
