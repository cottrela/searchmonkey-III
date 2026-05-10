use crate::plugins::cache::{
    validate_generated_text_cache, validate_generated_text_standalone, CacheStatus,
};
use crate::plugins::classifier::is_sm_text;
use crate::plugins::index_paths::source_path_from_mirror_text_path;
use crate::plugins::meta::{RangeContext, SmRange, SmRangeType};
use crate::plugins::registry::PluginRegistry;
use crate::search::SearchMatch;
use std::path::Path;

pub fn map_search_match(
    match_result: SearchMatch,
    registry: &PluginRegistry,
) -> Option<SearchMatch> {
    let path = Path::new(&match_result.path);
    if !is_sm_text(path) {
        return Some(match_result);
    }

    let source_path = source_path_from_mirror_text_path(path)?;
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_ascii_lowercase()))?;
    let plugin = registry.plugin_for_extension(&extension);
    let validation = match plugin {
        Some(plugin) => validate_generated_text_cache(path, plugin),
        None => validate_generated_text_standalone(path),
    };
    if validation.status != CacheStatus::Ready {
        return None;
    }

    let context = match_result
        .absolute_offset
        .and_then(|offset| validation.meta.as_ref()?.context_for_offset(offset))
        .map(format_range_context);

    Some(SearchMatch {
        path: source_path.to_string_lossy().to_string(),
        preview_path: Some(validation.text_path.to_string_lossy().to_string()),
        display_context: context,
        plugin_id: Some(
            plugin
                .map(|plugin| plugin.id.clone())
                .or_else(|| {
                    validation
                        .meta
                        .as_ref()
                        .map(|meta| meta.generator.plugin_id.clone())
                })
                .unwrap_or_default(),
        ),
        ..match_result
    })
}

fn format_range_context(context: RangeContext) -> String {
    let mut parts = Vec::new();

    if let Some(page) = context.page.and_then(format_page) {
        parts.push(page);
    }

    if context.smallest.kind != SmRangeType::Page {
        parts.push(format_smallest_range(&context.smallest));
    }

    parts.join(" · ")
}

fn format_page(page: SmRange) -> Option<String> {
    page.page.map(|page_number| format!("Page {page_number}"))
}

fn format_smallest_range(range: &SmRange) -> String {
    let label = range
        .label
        .as_ref()
        .filter(|label| !label.trim().is_empty())
        .cloned();
    if let Some(label) = label {
        return label;
    }

    let base = match range.kind {
        SmRangeType::Document => "Document",
        SmRangeType::Page => "Page",
        SmRangeType::Section => "Section",
        SmRangeType::Heading => "Heading",
        SmRangeType::Paragraph => "Paragraph",
        SmRangeType::Block => "Block",
        SmRangeType::PageBreak => "Page break",
        SmRangeType::ListItem => "List item",
        SmRangeType::Table => "Table",
        SmRangeType::Row => "Row",
        SmRangeType::Cell => "Cell",
        SmRangeType::Footnote => "Footnote",
        SmRangeType::Annotation => "Annotation",
        SmRangeType::ImageAlt => "Image alt",
    };

    if let Some(index) = range.index {
        format!("{base} {}", index + 1)
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::map_search_match;
    use crate::plugins::manifest::{PluginCapabilities, PluginPermission};
    use crate::plugins::registry::{PluginRegistry, RegisteredPlugin};
    use crate::search::{SearchMatch, SearchSubmatch};
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    #[test]
    fn remaps_generated_text_match_to_source_path() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();
        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "{}",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "1.2.3"
                  }},
                  "text": {{
                    "path": "{}",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }},
                    {{ "type": "block", "start": 0, "end": 11, "index": 2 }}
                  ]
                }}"#,
                source_path.display(),
                source_mtime,
                text_path.display()
            ),
        )
        .unwrap();

        let plugin = RegisteredPlugin {
            id: "sm.plugin.pdf".to_string(),
            name: "PDF Plugin".to_string(),
            version: "1.2.3".to_string(),
            root_dir: PathBuf::from("/plugins/sm.plugin.pdf"),
            command: PathBuf::from("/plugins/sm.plugin.pdf/bin/linux-x64/sm-plugin-pdf"),
            args: vec![],
            handles: vec![".pdf".to_string()],
            requires_entitlement: false,
            timeout_seconds: 60,
            capabilities: PluginCapabilities::default(),
            permissions: vec![
                PluginPermission::ReadSourceFile,
                PluginPermission::WriteSmOutputs,
            ],
        };
        let registry = PluginRegistry {
            by_id: HashMap::from([(plugin.id.clone(), plugin.clone())]),
            by_extension: HashMap::from([(".pdf".to_string(), vec![plugin.id.clone()])]),
            ignored_paths: HashSet::new(),
        };

        let mapped = map_search_match(
            SearchMatch {
                path: text_path.to_string_lossy().to_string(),
                preview_path: None,
                display_context: None,
                plugin_id: None,
                line_number: 1,
                line_text: "hello world".to_string(),
                submatches: vec![SearchSubmatch { start: 0, end: 5 }],
                absolute_offset: Some(0),
                file_size: None,
                modified_secs: None,
            },
            &registry,
        )
        .unwrap();

        assert_eq!(mapped.path, source_path.to_string_lossy());
        assert_eq!(mapped.preview_path.unwrap(), text_path.to_string_lossy());
        assert_eq!(mapped.display_context.unwrap(), "Page 8 · Block 3");
        assert_eq!(mapped.plugin_id.unwrap(), "sm.plugin.pdf");
    }

    #[test]
    fn remaps_generated_text_match_without_installed_plugin() {
        let temp = tempdir().unwrap();
        let source_path = temp.path().join("report.pdf");
        let text_path = temp.path().join("report.pdf.sm.txt");
        let meta_path = temp.path().join("report.pdf.sm.meta");

        fs::write(&source_path, b"pdf").unwrap();
        fs::write(&text_path, b"hello world").unwrap();
        let source_mtime =
            OffsetDateTime::from(fs::metadata(&source_path).unwrap().modified().unwrap())
                .format(&Rfc3339)
                .unwrap();
        fs::write(
            &meta_path,
            format!(
                r#"{{
                  "schema": "sm.meta.v1",
                  "source": {{
                    "path": "{}",
                    "size": 3,
                    "mtime": "{}"
                  }},
                  "generator": {{
                    "plugin_id": "sm.plugin.pdf",
                    "plugin_version": "9.9.9"
                  }},
                  "text": {{
                    "path": "{}",
                    "encoding": "utf-8",
                    "length_bytes": 11
                  }},
                  "ranges": [
                    {{ "type": "page", "start": 0, "end": 11, "page": 8 }}
                  ]
                }}"#,
                source_path.display(),
                source_mtime,
                text_path.display()
            ),
        )
        .unwrap();

        let registry = PluginRegistry::default();
        let mapped = map_search_match(
            SearchMatch {
                path: text_path.to_string_lossy().to_string(),
                preview_path: None,
                display_context: None,
                plugin_id: None,
                line_number: 1,
                line_text: "hello world".to_string(),
                submatches: vec![SearchSubmatch { start: 0, end: 5 }],
                absolute_offset: Some(0),
                file_size: None,
                modified_secs: None,
            },
            &registry,
        )
        .unwrap();

        assert_eq!(mapped.path, source_path.to_string_lossy());
        assert_eq!(mapped.preview_path.unwrap(), text_path.to_string_lossy());
        assert_eq!(mapped.plugin_id.unwrap(), "sm.plugin.pdf");
    }
}
