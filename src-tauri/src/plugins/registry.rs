use crate::plugins::manifest::{
    current_platform, PluginCapabilities, PluginManifest, PluginPermission, PluginPlatform,
};
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use semver::Version;
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
    pub versions_by_id: HashMap<String, Vec<RegisteredPlugin>>,
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
        Self::discover_for_platform_with_preferences(plugin_roots, platform, &HashMap::new())
    }

    pub fn discover_for_platform(
        plugin_roots: &[PathBuf],
        platform: PluginPlatform,
    ) -> Result<PluginDiscoveryReport> {
        Self::discover_for_platform_with_preferences(plugin_roots, platform, &HashMap::new())
    }

    pub fn discover_for_platform_with_preferences(
        plugin_roots: &[PathBuf],
        platform: PluginPlatform,
        preferred_versions: &HashMap<String, String>,
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
                        report
                            .registry
                            .versions_by_id
                            .entry(plugin.id.clone())
                            .or_default()
                            .push(plugin);
                    }
                    Err(err) => report.issues.push(PluginDiscoveryIssue {
                        manifest_path,
                        message: err.to_string(),
                    }),
                }
            }
        }

        for (plugin_id, versions) in &mut report.registry.versions_by_id {
            versions.sort_by(|left, right| plugin_version_cmp(&right.version, &left.version));
            if let Some(active) = select_active_plugin(versions, preferred_versions.get(plugin_id)) {
                report.registry.by_id.insert(plugin_id.clone(), active.clone());
            }
        }

        for plugin in report.registry.by_id.values() {
            for handle in &plugin.handles {
                report
                    .registry
                    .by_extension
                    .entry(handle.clone())
                    .or_default()
                    .push(plugin.id.clone());
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

pub fn plugin_version_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    match (Version::parse(left), Version::parse(right)) {
        (Ok(left_version), Ok(right_version)) => left_version.cmp(&right_version),
        _ => left.cmp(right),
    }
}

fn select_active_plugin<'a>(
    versions: &'a [RegisteredPlugin],
    preferred_version: Option<&String>,
) -> Option<&'a RegisteredPlugin> {
    if let Some(preferred_version) = preferred_version {
      if let Some(plugin) = versions.iter().find(|plugin| plugin.version == *preferred_version) {
          return Some(plugin);
      }
    }
    versions.first()
}

pub fn plugin_version_satisfies_selected(selected_version: &str, cached_version: &str) -> bool {
    match (Version::parse(selected_version), Version::parse(cached_version)) {
        (Ok(selected), Ok(cached)) => cached >= selected,
        _ => selected_version == cached_version,
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
    fn prefers_newest_semver_for_duplicate_plugin_ids() {
        let temp = tempdir().unwrap();
        let plugin_root_v1 = temp.path().join("sm.plugin.pdf/0.1.0");
        let plugin_root_v2 = temp.path().join("sm.plugin.pdf/0.1.1");

        for (root, version) in [(&plugin_root_v1, "0.1.0"), (&plugin_root_v2, "0.1.1")] {
            fs::create_dir_all(root.join("bin")).unwrap();
            fs::write(
                root.join("plugin.toml"),
                format!(
                    r#"
schema = "sm.plugin.v1"
id = "sm.plugin.pdf"
name = "PDF Plugin"
version = "{version}"
handles = [".pdf"]

[entry]
kind = "process"
command = "sm-plugin-pdf"
"#
                ),
            )
            .unwrap();
            fs::write(root.join("bin/sm-plugin-pdf"), "").unwrap();
        }

        let report = PluginRegistry::discover_for_platform(
            &[temp.path().to_path_buf()],
            PluginPlatform::LinuxX64,
        )
        .unwrap();

        let plugin = report.registry.by_id.get("sm.plugin.pdf").unwrap();
        assert_eq!(plugin.version, "0.1.1");
        assert_eq!(plugin.root_dir, plugin_root_v2);
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
