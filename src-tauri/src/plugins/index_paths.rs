use std::path::{Component, Path, PathBuf};

pub fn default_index_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(&home).join(".local/share/searchmonkey-3/index"));
            roots.push(
                PathBuf::from(&home).join("Library/Application Support/Searchmonkey-3/index"),
            );
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            roots.push(PathBuf::from(appdata).join("Searchmonkey-3").join("index"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
            roots.push(PathBuf::from(data_home).join("searchmonkey-3/index"));
        } else if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(home).join(".local/share/searchmonkey-3/index"));
        }
    }

    roots.dedup();
    roots
}

pub fn default_state_db_path() -> Option<PathBuf> {
    default_index_roots()
        .into_iter()
        .next()
        .and_then(|index_root| {
            index_root
                .parent()
                .map(|parent| parent.join("searchmonkey.sqlite"))
        })
}

pub fn mirror_relative_path(source_path: &Path) -> PathBuf {
    let mut relative = PathBuf::new();

    for component in source_path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            Component::ParentDir => relative.push(".."),
            Component::Prefix(prefix) => {
                let sanitized = prefix.as_os_str().to_string_lossy().replace(':', "");
                if !sanitized.is_empty() {
                    relative.push(sanitized);
                }
            }
        }
    }

    relative
}

pub fn mirror_text_path(index_root: &Path, source_path: &Path) -> PathBuf {
    let source_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    index_root
        .join(mirror_relative_path(source_path))
        .with_file_name(format!("{source_name}.sm.txt"))
}

pub fn mirror_meta_path(index_root: &Path, source_path: &Path) -> PathBuf {
    let source_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    index_root
        .join(mirror_relative_path(source_path))
        .with_file_name(format!("{source_name}.sm.meta"))
}

pub fn mirror_text_tmp_path(index_root: &Path, source_path: &Path) -> PathBuf {
    let source_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    index_root
        .join(mirror_relative_path(source_path))
        .with_file_name(format!("{source_name}.sm.txt.tmp"))
}

pub fn mirror_meta_tmp_path(index_root: &Path, source_path: &Path) -> PathBuf {
    let source_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    index_root
        .join(mirror_relative_path(source_path))
        .with_file_name(format!("{source_name}.sm.meta.tmp"))
}

pub fn mirror_failure_state_path(index_root: &Path, source_path: &Path) -> PathBuf {
    let source_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    index_root
        .join(mirror_relative_path(source_path))
        .with_file_name(format!("{source_name}.sm.state"))
}

pub fn mirror_search_path(index_root: &Path, search_path: &Path) -> PathBuf {
    index_root.join(mirror_relative_path(search_path))
}

pub fn source_path_from_mirror_text_path(text_path: &Path) -> Option<PathBuf> {
    if let Some(index_root) = default_index_roots()
        .into_iter()
        .find(|root| text_path.starts_with(root))
    {
        return source_path_from_mirror_text_path_with_root(text_path, &index_root);
    }

    #[cfg(test)]
    {
        let file_name = text_path.file_name()?.to_str()?;
        let source_name = file_name.strip_suffix(".sm.txt")?;
        return Some(text_path.with_file_name(source_name));
    }

    #[cfg(not(test))]
    {
        None
    }
}

pub fn source_path_from_mirror_text_path_with_root(
    text_path: &Path,
    index_root: &Path,
) -> Option<PathBuf> {
    let relative = text_path.strip_prefix(index_root).ok()?;
    let file_name = relative.file_name()?.to_str()?;
    let source_name = file_name.strip_suffix(".sm.txt")?;

    #[cfg(windows)]
    {
        let mut components = relative.components();
        let drive = components.next()?.as_os_str().to_string_lossy().to_string();
        let mut source = PathBuf::from(format!("{drive}:\\"));
        for component in components {
            source.push(component.as_os_str());
        }
        source.set_file_name(source_name);
        Some(source)
    }

    #[cfg(not(windows))]
    {
        Some(Path::new("/").join(relative).with_file_name(source_name))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        mirror_failure_state_path, mirror_meta_path, mirror_meta_tmp_path, mirror_search_path,
        mirror_text_path, mirror_text_tmp_path, source_path_from_mirror_text_path_with_root,
    };
    use std::path::PathBuf;

    #[test]
    fn maps_unix_source_paths_into_mirror_paths() {
        let root = PathBuf::from("/index");
        let source = PathBuf::from("/Users/acottrell/sm-test/valid.pdf");

        assert_eq!(
            mirror_text_path(&root, &source),
            PathBuf::from("/index/Users/acottrell/sm-test/valid.pdf.sm.txt")
        );
        assert_eq!(
            mirror_meta_path(&root, &source),
            PathBuf::from("/index/Users/acottrell/sm-test/valid.pdf.sm.meta")
        );
        assert_eq!(
            mirror_text_tmp_path(&root, &source),
            PathBuf::from("/index/Users/acottrell/sm-test/valid.pdf.sm.txt.tmp")
        );
        assert_eq!(
            mirror_meta_tmp_path(&root, &source),
            PathBuf::from("/index/Users/acottrell/sm-test/valid.pdf.sm.meta.tmp")
        );
        assert_eq!(
            mirror_failure_state_path(&root, &source),
            PathBuf::from("/index/Users/acottrell/sm-test/valid.pdf.sm.state")
        );
        assert_eq!(
            mirror_search_path(&root, PathBuf::from("/Users/acottrell/sm-test").as_path()),
            PathBuf::from("/index/Users/acottrell/sm-test")
        );
    }

    #[test]
    fn recovers_unix_source_path_from_mirror_text_path() {
        let root = PathBuf::from("/index");
        let text = PathBuf::from("/index/Users/acottrell/sm-test/valid.pdf.sm.txt");

        assert_eq!(
            source_path_from_mirror_text_path_with_root(&text, &root).unwrap(),
            PathBuf::from("/Users/acottrell/sm-test/valid.pdf")
        );
    }
}
