use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use url::Url;

const SM_PLUGIN_SCHEMA: &str = "sm.plugin.v1";
const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub homepage: Option<String>,
    pub handles: Vec<String>,
    pub entry: PluginEntry,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub permissions: Vec<PluginPermission>,
    #[serde(default)]
    pub requires_entitlement: bool,
    pub timeout_seconds: Option<u64>,
    pub platforms: Option<Vec<PluginPlatform>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginEntry {
    pub kind: PluginEntryKind,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginEntryKind {
    Process,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash)]
pub enum PluginPlatform {
    #[serde(rename = "linux-x64")]
    LinuxX64,
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
    #[serde(rename = "windows-x64")]
    WindowsX64,
    #[serde(rename = "macos-x64")]
    MacOsX64,
    #[serde(rename = "macos-arm64")]
    MacOsArm64,
}

impl PluginPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::WindowsX64 => "windows-x64",
            Self::MacOsX64 => "macos-x64",
            Self::MacOsArm64 => "macos-arm64",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub text: bool,
    #[serde(default)]
    pub layout: bool,
    #[serde(default)]
    pub ocr: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    ReadSourceFile,
    WriteSmOutputs,
}

impl PluginManifest {
    pub fn parse_str(contents: &str) -> Result<Self> {
        let manifest: Self =
            toml::from_str(contents).context("failed to parse plugin.toml as TOML")?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed reading plugin manifest at {}", path.display()))?;
        Self::parse_str(&contents)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != SM_PLUGIN_SCHEMA {
            bail!(
                "unsupported plugin schema {:?}; expected {}",
                self.schema,
                SM_PLUGIN_SCHEMA
            );
        }
        if !is_valid_plugin_id(&self.id) {
            bail!("plugin id must match ^[a-z0-9.-]+$");
        }
        if self.name.trim().is_empty() {
            bail!("plugin name must not be empty");
        }
        if self.version.trim().is_empty() {
            bail!("plugin version must not be empty");
        }
        if let Some(homepage) = &self.homepage {
            Url::parse(homepage).context("plugin homepage must be a valid URI")?;
        }
        if self.handles.is_empty() {
            bail!("plugin handles must not be empty");
        }
        for handle in &self.handles {
            if !is_valid_extension(handle) {
                bail!("plugin handle {handle:?} must match ^\\.[a-z0-9]+$");
            }
        }
        if self.entry.kind != PluginEntryKind::Process {
            bail!("only process entry kinds are supported");
        }
        if self.entry.command.trim().is_empty() {
            bail!("entry.command must not be empty");
        }
        if let Some(timeout_seconds) = self.timeout_seconds {
            if timeout_seconds == 0 || timeout_seconds > 3600 {
                bail!("timeout_seconds must be between 1 and 3600");
            }
        }

        Ok(())
    }

    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS)
    }

    pub fn supports_platform(&self, platform: PluginPlatform) -> bool {
        self.platforms
            .as_ref()
            .is_none_or(|platforms| platforms.contains(&platform))
    }
}

pub fn current_platform() -> Result<PluginPlatform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok(PluginPlatform::LinuxX64),
        ("linux", "aarch64") => Ok(PluginPlatform::LinuxArm64),
        ("windows", "x86_64") => Ok(PluginPlatform::WindowsX64),
        ("macos", "x86_64") => Ok(PluginPlatform::MacOsX64),
        ("macos", "aarch64") => Ok(PluginPlatform::MacOsArm64),
        (os, arch) => bail!("unsupported plugin platform {os}-{arch}"),
    }
}

fn is_valid_plugin_id(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
}

fn is_valid_extension(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('.') else {
        return false;
    };

    !rest.is_empty()
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{PluginEntryKind, PluginManifest, PluginPermission};

    #[test]
    fn parses_valid_plugin_manifest() {
        let manifest = PluginManifest::parse_str(
            r#"
schema = "sm.plugin.v1"
id = "sm.plugin.pdf"
name = "PDF Plugin"
version = "1.2.3"
handles = [".pdf"]
permissions = ["read_source_file", "write_sm_outputs"]
timeout_seconds = 45

[entry]
kind = "process"
command = "sm-plugin-pdf"
args = ["--job"]

[capabilities]
text = true
layout = true
"#,
        )
        .unwrap();

        assert_eq!(manifest.entry.kind, PluginEntryKind::Process);
        assert_eq!(manifest.timeout_seconds(), 45);
        assert_eq!(manifest.permissions.len(), 2);
        assert_eq!(manifest.permissions[0], PluginPermission::ReadSourceFile);
    }

    #[test]
    fn rejects_invalid_extension_handle() {
        let error = PluginManifest::parse_str(
            r#"
schema = "sm.plugin.v1"
id = "sm.plugin.pdf"
name = "PDF Plugin"
version = "1.2.3"
handles = ["pdf"]

[entry]
kind = "process"
command = "sm-plugin-pdf"
"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("must match"));
    }
}
