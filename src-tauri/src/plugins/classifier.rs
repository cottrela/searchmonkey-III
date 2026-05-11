use crate::plugins::registry::PluginRegistry;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const DEFAULT_TEXT_EXTENSIONS: &[&str] = &[
    "c", "cpp", "css", "csv", "go", "h", "html", "java", "js", "json", "jsonl", "jsx", "log", "md",
    "mjs", "py", "rb", "rs", "sh", "sql", "svg", "svelte", "toml", "ts", "tsx", "txt", "xml",
    "yaml", "yml",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    NativeText,
    GeneratedSmText,
    SmMeta,
    PluginBinary,
    PluginManifest,
    SupportedByPlugin { plugin_id: String },
    UnsupportedBinary,
    Ignored,
}

#[derive(Debug, Clone)]
pub struct FileClassifier {
    known_text_extensions: HashSet<String>,
    plugin_roots: Vec<PathBuf>,
    registered_extensions: std::collections::HashMap<String, String>,
}

impl FileClassifier {
    pub fn new(registry: &PluginRegistry) -> Self {
        Self::with_known_text_extensions(
            registry,
            DEFAULT_TEXT_EXTENSIONS.iter().copied().map(str::to_string),
        )
    }

    pub fn with_known_text_extensions<I>(registry: &PluginRegistry, extensions: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let registered_extensions = registry
            .by_extension
            .iter()
            .filter_map(|(extension, plugin_ids)| {
                plugin_ids
                    .first()
                    .map(|plugin_id| (extension.clone(), plugin_id.clone()))
            })
            .collect();

        Self {
            known_text_extensions: extensions
                .into_iter()
                .map(|value| value.to_ascii_lowercase())
                .collect(),
            plugin_roots: registry.ignored_paths.iter().cloned().collect(),
            registered_extensions,
        }
    }

    pub fn classify(&self, path: &Path) -> FileKind {
        if self.is_in_plugin_root(path) {
            return self.classify_plugin_internal(path);
        }

        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            return FileKind::Ignored;
        };

        if file_name.ends_with(".sm.txt.tmp") || file_name.ends_with(".sm.meta.tmp") {
            return FileKind::Ignored;
        }
        if file_name.ends_with(".sm.meta") {
            return FileKind::SmMeta;
        }
        if file_name.ends_with(".sm.txt") {
            return FileKind::GeneratedSmText;
        }

        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{}", value.to_ascii_lowercase()));

        if let Some(extension) = extension {
            if let Some(plugin_id) = self.registered_extensions.get(&extension) {
                return FileKind::SupportedByPlugin {
                    plugin_id: plugin_id.clone(),
                };
            }

            if self
                .known_text_extensions
                .contains(extension.trim_start_matches('.'))
            {
                return FileKind::NativeText;
            }

            return FileKind::UnsupportedBinary;
        }

        FileKind::NativeText
    }

    pub fn is_searchable(&self, path: &Path) -> bool {
        matches!(
            self.classify(path),
            FileKind::NativeText | FileKind::GeneratedSmText
        )
    }

    fn is_in_plugin_root(&self, path: &Path) -> bool {
        self.plugin_roots.iter().any(|root| path.starts_with(root))
    }

    fn classify_plugin_internal(&self, path: &Path) -> FileKind {
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            return FileKind::Ignored;
        };

        if file_name == "plugin.toml" {
            return FileKind::PluginManifest;
        }

        if path
            .components()
            .any(|component| component.as_os_str().to_string_lossy() == "bin")
        {
            return FileKind::PluginBinary;
        }

        FileKind::Ignored
    }
}

pub fn is_sm_text(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.ends_with(".sm.txt"))
}

pub fn source_for_sm_text(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    let source_name = file_name.strip_suffix(".sm.txt")?;
    Some(path.with_file_name(source_name))
}

pub fn meta_for_sm_text(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    let source_name = file_name.strip_suffix(".sm.txt")?;
    Some(path.with_file_name(format!("{source_name}.sm.meta")))
}

#[cfg(test)]
mod tests {
    use super::{is_sm_text, meta_for_sm_text, source_for_sm_text, FileClassifier, FileKind};
    use crate::plugins::registry::PluginRegistry;
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    #[test]
    fn classifies_generated_files_and_plugin_supported_sources() {
        let registry = PluginRegistry {
            by_id: HashMap::new(),
            versions_by_id: HashMap::new(),
            by_extension: HashMap::from([(".pdf".to_string(), vec!["sm.plugin.pdf".to_string()])]),
            ignored_paths: HashSet::from([PathBuf::from("/plugins/sm.plugin.pdf")]),
        };
        let classifier = FileClassifier::new(&registry);

        assert_eq!(
            classifier.classify(PathBuf::from("/docs/report.pdf.sm.txt").as_path()),
            FileKind::GeneratedSmText
        );
        assert_eq!(
            classifier.classify(PathBuf::from("/docs/report.pdf.sm.meta").as_path()),
            FileKind::SmMeta
        );
        assert_eq!(
            classifier.classify(PathBuf::from("/docs/report.pdf").as_path()),
            FileKind::SupportedByPlugin {
                plugin_id: "sm.plugin.pdf".to_string()
            }
        );
        assert_eq!(
            classifier.classify(PathBuf::from("/docs/readme.md").as_path()),
            FileKind::NativeText
        );
    }

    #[test]
    fn classifies_plugin_internal_files() {
        let registry = PluginRegistry {
            by_id: HashMap::new(),
            versions_by_id: HashMap::new(),
            by_extension: HashMap::new(),
            ignored_paths: HashSet::from([PathBuf::from("/plugins/sm.plugin.pdf")]),
        };
        let classifier = FileClassifier::new(&registry);

        assert_eq!(
            classifier.classify(PathBuf::from("/plugins/sm.plugin.pdf/plugin.toml").as_path()),
            FileKind::PluginManifest
        );
        assert_eq!(
            classifier.classify(
                PathBuf::from("/plugins/sm.plugin.pdf/bin/linux-x64/sm-plugin-pdf").as_path()
            ),
            FileKind::PluginBinary
        );
        assert_eq!(
            classifier
                .classify(PathBuf::from("/plugins/sm.plugin.pdf/licenses/POPPLER.txt").as_path()),
            FileKind::Ignored
        );
    }

    #[test]
    fn maps_sm_text_back_to_source_and_meta() {
        let path = PathBuf::from("/docs/report.pdf.sm.txt");
        assert!(is_sm_text(&path));
        assert_eq!(
            source_for_sm_text(&path).unwrap(),
            PathBuf::from("/docs/report.pdf")
        );
        assert_eq!(
            meta_for_sm_text(&path).unwrap(),
            PathBuf::from("/docs/report.pdf.sm.meta")
        );
    }
}
