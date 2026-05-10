use crate::plugins::manifest::{
    current_platform, PluginCapabilities, PluginManifest, PluginPermission, PluginPlatform,
};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const PLUGIN_MANIFEST_FILE: &str = "plugin.toml";

#[derive(Debug, Clone)]
pub struct RegisteredPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub root_dir: PathBuf,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub handles: Vec<String>,
    pub requires_entitlement: bool,
    pub timeout_seconds: u64,
    pub capabilities: PluginCapabilities,
    pub permissions: Vec<PluginPermission>,
}

#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    pub by_id: HashMap<String, RegisteredPlugin>,
    pub by_extension: HashMap<String, Vec<String>>,
    pub ignored_paths: HashSet<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PluginDiscoveryIssue {
    pub manifest_path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct PluginDiscoveryReport {
    pub registry: PluginRegistry,
    pub issues: Vec<PluginDiscoveryIssue>,
}

impl PluginRegistry {
    pub fn discover_default() -> Result<PluginDiscoveryReport> {
        Self::discover(&default_plugin_roots())
    }

    pub fn discover(plugin_roots: &[PathBuf]) -> Result<PluginDiscoveryReport> {
        let platform = current_platform()?;
        Self::discover_for_platform(plugin_roots, platform)
    }

    pub fn discover_for_platform(
        plugin_roots: &[PathBuf],
        platform: PluginPlatform,
    ) -> Result<PluginDiscoveryReport> {
        let mut report = PluginDiscoveryReport::default();

        for plugin_root in plugin_roots {
            if !plugin_root.exists() {
                continue;
            }
            for manifest_path in find_manifest_paths(plugin_root)? {
                let plugin_dir = manifest_path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| plugin_root.clone());
                report.registry.ignored_paths.insert(plugin_dir.clone());

                match register_plugin(&manifest_path, plugin_dir, platform) {
                    Ok(plugin) => {
                        for handle in &plugin.handles {
                            report
                                .registry
                                .by_extension
                                .entry(handle.clone())
                                .or_default()
                                .push(plugin.id.clone());
                        }
                        report.registry.by_id.insert(plugin.id.clone(), plugin);
                    }
                    Err(err) => report.issues.push(PluginDiscoveryIssue {
                        manifest_path,
                        message: err.to_string(),
                    }),
                }
            }
        }

        Ok(report)
    }

    pub fn plugin_for_extension(&self, extension: &str) -> Option<&RegisteredPlugin> {
        self.by_extension
            .get(extension)
            .and_then(|plugin_ids| plugin_ids.first())
            .and_then(|plugin_id| self.by_id.get(plugin_id))
    }
}

pub fn default_plugin_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(&home).join(".local/share/searchmonkey-3/plugins"));
            roots.push(PathBuf::from(&home).join("Library/Application Support/searchmonkey-3/plugins"));
            roots.push(PathBuf::from(home).join("Library/Application Support/Searchmonkey-3/plugins"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            roots.push(PathBuf::from(appdata).join("Searchmonkey-3").join("plugins"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
            roots.push(PathBuf::from(&data_home).join("searchmonkey-3/plugins"));
        }
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(&home).join(".local/share/searchmonkey-3/plugins"));
            roots.push(PathBuf::from(home).join(".config/searchmonkey-3/plugins"));
        }
    }

    roots.dedup();
    roots
}

fn find_manifest_paths(plugin_root: &Path) -> Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    let walker = WalkBuilder::new(plugin_root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .build();

    for entry in walker {
        let entry = entry.with_context(|| {
            format!(
                "failed while scanning plugin directory {}",
                plugin_root.display()
            )
        })?;
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        if entry.file_name() == PLUGIN_MANIFEST_FILE {
            manifests.push(entry.into_path());
        }
    }

    manifests.sort();
    Ok(manifests)
}

fn register_plugin(
    manifest_path: &Path,
    plugin_dir: PathBuf,
    platform: PluginPlatform,
) -> Result<RegisteredPlugin> {
    let manifest = PluginManifest::load(manifest_path)?;
    if !manifest.supports_platform(platform) {
        anyhow::bail!("plugin does not support platform {}", platform.as_str());
    }
    let timeout_seconds = manifest.timeout_seconds();

    let command = resolve_plugin_command(&plugin_dir, &manifest.entry.command, platform)?;

    Ok(RegisteredPlugin {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        root_dir: plugin_dir,
        command,
        args: manifest.entry.args,
        handles: manifest.handles,
        requires_entitlement: manifest.requires_entitlement,
        timeout_seconds,
        capabilities: manifest.capabilities,
        permissions: manifest.permissions,
    })
}

fn resolve_plugin_command(
    plugin_dir: &Path,
    command_name: &str,
    platform: PluginPlatform,
) -> Result<PathBuf> {
    let flat_command = plugin_dir.join("bin").join(command_name);
    if flat_command.is_file() {
        return Ok(flat_command);
    }

    let platform_command = plugin_dir
        .join("bin")
        .join(platform.as_str())
        .join(command_name);
    if platform_command.is_file() {
        return Ok(platform_command);
    }

    anyhow::bail!(
        "plugin entry binary is missing at {} or {}",
        flat_command.display(),
        platform_command.display()
    );
}

#[cfg(test)]
mod tests {
    use super::PluginRegistry;
    use crate::plugins::manifest::PluginPlatform;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn discovers_compatible_plugin_and_indexes_by_extension() {
        let temp = tempdir().unwrap();
        let plugin_root = temp.path().join("sm.plugin.pdf/0.1.0");
        fs::create_dir_all(plugin_root.join("bin")).unwrap();
        fs::write(
            plugin_root.join("plugin.toml"),
            r#"
schema = "sm.plugin.v1"
id = "sm.plugin.pdf"
name = "PDF Plugin"
version = "1.2.3"
handles = [".pdf"]
requires_entitlement = true

[entry]
kind = "process"
command = "sm-plugin-pdf"
args = ["--job"]
"#,
        )
        .unwrap();
        fs::write(plugin_root.join("bin/sm-plugin-pdf"), "").unwrap();

        let report = PluginRegistry::discover_for_platform(
            &[temp.path().to_path_buf()],
            PluginPlatform::LinuxX64,
        )
        .unwrap();

        assert!(report.issues.is_empty());
        let plugin = report.registry.by_id.get("sm.plugin.pdf").unwrap();
        assert_eq!(plugin.handles, vec![".pdf"]);
        assert!(plugin.requires_entitlement);
        assert_eq!(
            report.registry.by_extension.get(".pdf").unwrap(),
            &vec!["sm.plugin.pdf".to_string()]
        );
        assert!(report.registry.ignored_paths.contains(&plugin_root));
    }

    #[test]
    fn records_issue_for_missing_binary() {
        let temp = tempdir().unwrap();
        let plugin_root = temp.path().join("sm.plugin.pdf/0.1.0");
        fs::create_dir_all(&plugin_root).unwrap();
        fs::write(
            plugin_root.join("plugin.toml"),
            r#"
schema = "sm.plugin.v1"
id = "sm.plugin.pdf"
name = "PDF Plugin"
version = "1.2.3"
handles = [".pdf"]

[entry]
kind = "process"
command = "sm-plugin-pdf"
"#,
        )
        .unwrap();

        let report = PluginRegistry::discover_for_platform(
            &[temp.path().to_path_buf()],
            PluginPlatform::LinuxX64,
        )
        .unwrap();

        assert!(report.registry.by_id.is_empty());
        assert_eq!(report.issues.len(), 1);
        assert!(report.issues[0].message.contains("missing"));
    }
}
