use crate::plugins::registry::PluginRegistry;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    exclude_globs: Vec<String>,
}

impl SearchFilter {
    pub fn for_search_root(search_root: &Path, registry: &PluginRegistry) -> Self {
        let mut exclude_globs = vec![
            "!**/*.sm.meta".to_string(),
            "!**/*.sm.txt.tmp".to_string(),
            "!**/*.sm.meta.tmp".to_string(),
            "!**/plugin.toml".to_string(),
        ];

        let mut plugin_extensions = registry.by_extension.keys().cloned().collect::<Vec<_>>();
        plugin_extensions.sort();
        for extension in plugin_extensions {
            exclude_globs.push(format!("!**/*{extension}"));
        }

        let mut plugin_roots = registry.ignored_paths.iter().cloned().collect::<Vec<_>>();
        plugin_roots.sort();
        for plugin_root in plugin_roots {
            if let Some(relative) = relative_glob(search_root, &plugin_root) {
                exclude_globs.push(format!("!{relative}/**"));
            }
        }

        Self { exclude_globs }
    }

    pub fn exclude_globs(&self) -> &[String] {
        &self.exclude_globs
    }

    pub fn apply_to_args(&self, args: &mut Vec<String>) {
        for pattern in &self.exclude_globs {
            args.push("--glob".to_string());
            args.push(pattern.clone());
        }
    }
}

fn relative_glob(search_root: &Path, plugin_root: &Path) -> Option<String> {
    let relative = plugin_root.strip_prefix(search_root).ok()?;
    let normalized = normalize_path(relative);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::SearchFilter;
    use crate::plugins::registry::PluginRegistry;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    #[test]
    fn builds_expected_excludes() {
        let registry = PluginRegistry {
            by_id: HashMap::new(),
            by_extension: HashMap::from([(".pdf".to_string(), vec!["sm.plugin.pdf".to_string()])]),
            ignored_paths: HashSet::from([PathBuf::from(
                "/home/user/.config/searchmonkey-3/plugins/sm.plugin.pdf",
            )]),
        };

        let filter =
            SearchFilter::for_search_root(PathBuf::from("/home/user").as_path(), &registry);
        let excludes = filter.exclude_globs();

        assert!(excludes.contains(&"!**/*.sm.meta".to_string()));
        assert!(excludes.contains(&"!**/*".to_string()) == false);
        assert!(excludes.contains(&"!**/*.pdf".to_string()));
        assert!(excludes.contains(&"!.config/searchmonkey-3/plugins/sm.plugin.pdf/**".to_string()));
    }
}
