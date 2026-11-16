use std::fs;
use std::path::{Path, PathBuf};

pub fn model_slug(id: &str) -> String {
    id.replace(':', "-")
}

pub fn find_model_directory(root: &Path, slug: &str) -> Option<PathBuf> {
    let direct = root.join(slug);

    if direct.is_dir() {
        return Some(direct);
    }

    let entries = fs::read_dir(root).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let candidate = path.join(slug);

        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    None
}

pub fn model_directory_size(root: &Path, slug: &str) -> u64 {
    find_model_directory(root, slug)
        .map(|directory| directory_size(&directory))
        .unwrap_or_default()
}

pub fn directory_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };

    let mut total = 0;

    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        if metadata.is_dir() {
            total += directory_size(&entry.path());
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }

    total
}

pub fn remove_model_directory(root: &Path, slug: &str) {
    let Some(directory) = find_model_directory(root, slug) else {
        return;
    };
    let publisher = directory.parent().map(Path::to_path_buf);

    if fs::remove_dir_all(&directory).is_ok()
        && let Some(publisher) = publisher
    {
        let _ = fs::remove_dir(publisher);
    }
}

const MAX_PRUNE_DEPTH: usize = 4;

pub fn prune_empty_directories(root: &Path) {
    prune_empty_directories_at(root, 0);
}

fn prune_empty_directories_at(path: &Path, depth: usize) {
    if depth >= MAX_PRUNE_DEPTH {
        return;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    for entry in entries.flatten() {
        let child = entry.path();

        if child.is_dir() {
            prune_empty_directories_at(&child, depth + 1);
            let _ = fs::remove_dir(&child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent directory");
        }

        fs::write(path, vec![0u8; bytes]).expect("file is written");
    }

    #[test]
    fn replaces_the_version_separator() {
        assert_eq!(
            model_slug("openai-whisper-tiny-generic-cpu:4"),
            "openai-whisper-tiny-generic-cpu-4"
        );
    }

    #[test]
    fn finds_the_model_inside_its_publisher_directory() {
        let root = tempfile::tempdir().expect("cache root");
        let model = root
            .path()
            .join("Microsoft")
            .join("openai-whisper-tiny-generic-cpu-4");

        fs::create_dir_all(model.join("v4")).expect("model directory");

        assert_eq!(
            find_model_directory(root.path(), "openai-whisper-tiny-generic-cpu-4"),
            Some(model)
        );
        assert_eq!(find_model_directory(root.path(), "missing"), None);
    }

    #[test]
    fn sums_nested_file_sizes() {
        let root = tempfile::tempdir().expect("cache root");
        let model = root.path().join("publisher").join("model-1");

        write_file(&model.join("v1").join("weights.onnx"), 120);
        write_file(&model.join("v1").join("config.json"), 30);

        assert_eq!(model_directory_size(root.path(), "model-1"), 150);
        assert_eq!(model_directory_size(root.path(), "missing"), 0);
    }

    #[test]
    fn removes_a_partial_model_directory() {
        let root = tempfile::tempdir().expect("cache root");
        let model = root.path().join("publisher").join("model-1");

        write_file(&model.join("v1").join("weights.onnx"), 10);

        remove_model_directory(root.path(), "model-1");

        assert!(!model.exists());
        assert!(!root.path().join("publisher").exists());
    }

    #[test]
    fn prunes_only_empty_directories() {
        let root = tempfile::tempdir().expect("cache root");
        let empty = root.path().join("publisher").join("empty-1");
        let used = root.path().join("publisher").join("used-1");

        fs::create_dir_all(&empty).expect("empty directory");
        write_file(&used.join("v1").join("weights.onnx"), 10);

        prune_empty_directories(root.path());

        assert!(!empty.exists());
        assert!(used.exists());
        assert!(root.path().exists());
    }
}
