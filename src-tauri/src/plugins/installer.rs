use crate::plugins::manifest::{current_platform, PluginManifest};
use anyhow::{bail, Context, Result};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

const PLUGIN_MANIFEST_FILE: &str = "plugin.toml";

#[derive(Debug, Clone)]
pub struct InstalledPluginPackage {
    pub plugin_id: String,
    pub version: String,
    pub install_dir: PathBuf,
}

pub fn install_plugin_archive(
    archive_path: &Path,
    plugin_root: &Path,
) -> Result<InstalledPluginPackage> {
    if archive_path.extension().and_then(|value| value.to_str()) != Some("smplugin") {
        bail!("plugin package must use the .smplugin extension");
    }

    fs::create_dir_all(plugin_root)
        .with_context(|| format!("failed creating plugin root {}", plugin_root.display()))?;

    let temp_root = unique_temp_dir();
    fs::create_dir_all(&temp_root)
        .with_context(|| format!("failed creating temp directory {}", temp_root.display()))?;

    let install_result = (|| {
        unpack_archive(archive_path, &temp_root)?;
        let package_root = detect_package_root(&temp_root)?;
        let manifest_path = package_root.join(PLUGIN_MANIFEST_FILE);
        let manifest = PluginManifest::load(&manifest_path)?;
        let platform = current_platform()?;
        if !manifest.supports_platform(platform) {
            bail!("plugin does not support platform {}", platform.as_str());
        }

        let install_dir = plugin_root.join(&manifest.id).join(&manifest.version);
        if install_dir.exists() {
            fs::remove_dir_all(&install_dir).with_context(|| {
                format!(
                    "failed replacing existing install at {}",
                    install_dir.display()
                )
            })?;
        }
        if let Some(parent) = install_dir.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed creating plugin install directory {}",
                    parent.display()
                )
            })?;
        }
        copy_dir_all(&package_root, &install_dir)?;

        Ok(InstalledPluginPackage {
            plugin_id: manifest.id,
            version: manifest.version,
            install_dir,
        })
    })();

    let _ = fs::remove_dir_all(&temp_root);
    install_result
}

fn unpack_archive(archive_path: &Path, temp_root: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed opening plugin package {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed reading {}", archive_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let entry_path = safe_entry_path(entry.name())?;
        let output_path = temp_root.join(entry_path);

        if entry.is_dir() {
            fs::create_dir_all(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = fs::File::create(&output_path)?;
        io::copy(&mut entry, &mut output)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))?;
        }
    }

    Ok(())
}

fn safe_entry_path(name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        bail!("plugin archive contains an absolute path");
    }

    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => clean.push(value),
            Component::CurDir => {}
            _ => bail!("plugin archive contains an invalid path"),
        }
    }

    if clean.as_os_str().is_empty() {
        bail!("plugin archive contains an empty path");
    }

    Ok(clean)
}

fn detect_package_root(temp_root: &Path) -> Result<PathBuf> {
    let manifests = find_manifests(temp_root)?;
    if manifests.is_empty() {
        bail!("plugin archive does not contain plugin.toml");
    }
    if manifests.len() > 1 {
        bail!("plugin archive must contain exactly one plugin.toml");
    }

    let manifest_path = manifests.into_iter().next().expect("manifest exists");
    let package_root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .context("plugin manifest must live inside a directory")?;

    if package_root == temp_root {
        return Ok(package_root);
    }

    let wrapper = package_root
        .parent()
        .filter(|parent| *parent == temp_root)
        .map(|_| package_root.clone())
        .context("plugin archive must unpack to a single plugin directory")?;

    Ok(wrapper)
}

fn find_manifests(root: &Path) -> Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed reading extracted directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
                continue;
            }
            if entry.file_name() == PLUGIN_MANIFEST_FILE {
                manifests.push(path);
            }
        }
    }

    manifests.sort();
    Ok(manifests)
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed creating directory {}", destination.display()))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("failed reading source directory {}", source.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            fs::copy(&path, &target).with_context(|| {
                format!("failed copying {} to {}", path.display(), target.display())
            })?;
        }
    }

    Ok(())
}

fn unique_temp_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "searchmonkey-plugin-install-{timestamp}-{}",
        std::process::id()
    ))
}
