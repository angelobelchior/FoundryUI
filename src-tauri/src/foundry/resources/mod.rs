use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceErrorKind {
    Unavailable,
    AccessDenied,
    NotDirectory,
    UnsupportedPlatform,
    UnsupportedVolume,
    RootChanged,
    Revoked,
    InvalidPath,
    StrictContainmentUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[error("resource operation failed: {kind:?}")]
pub struct ResourceError {
    pub kind: ResourceErrorKind,
    pub raw_os_error: Option<i32>,
}

pub type ResourceResult<T> = Result<T, ResourceError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn parse(value: &str) -> ResourceResult<Self> {
        if value.is_empty()
            || value.len() > 4096
            || value.contains('\\')
            || value.chars().any(char::is_control)
            || value.as_bytes().get(1) == Some(&b':')
        {
            return Err(ResourceError {
                kind: ResourceErrorKind::InvalidPath,
                raw_os_error: None,
            });
        }
        let mut components = Vec::new();
        for component in Path::new(value).components() {
            match component {
                Component::Normal(name) => components.push(name.to_string_lossy().into_owned()),
                Component::CurDir => {}
                _ => {
                    return Err(ResourceError {
                        kind: ResourceErrorKind::InvalidPath,
                        raw_os_error: None,
                    });
                }
            }
        }
        if components.is_empty() {
            return Err(ResourceError {
                kind: ResourceErrorKind::InvalidPath,
                raw_os_error: None,
            });
        }
        Ok(Self(components.join("/")))
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RootIdentity {
    volume_uuid: [u8; 16],
    file_id: u64,
    generation: u32,
    birth_seconds: i64,
    birth_nanoseconds: i64,
}

impl RootIdentity {
    pub fn storage_key(&self) -> String {
        let volume: String = self
            .volume_uuid
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!(
            "darwin:{volume}:{:x}:{:x}:{:x}:{:x}",
            self.file_id, self.generation, self.birth_seconds, self.birth_nanoseconds
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RootRelation {
    Equivalent,
    Ancestor,
    Descendant,
    Disjoint,
    Indeterminate,
}

pub struct RootHandle {
    #[cfg(target_os = "macos")]
    file: std::fs::File,
    identity: RootIdentity,
    display_path: PathBuf,
    revoked: AtomicBool,
}

impl RootHandle {
    pub fn enroll(path: &Path) -> ResourceResult<Self> {
        #[cfg(target_os = "macos")]
        {
            let display_path = std::fs::canonicalize(path).map_err(macos::io_error)?;
            let file = macos::open_directory(&display_path)?;
            let identity = macos::identity(&file)?;
            let current = macos::open_directory(&display_path)?;
            if macos::identity(&current)? != identity {
                return Err(ResourceError {
                    kind: ResourceErrorKind::RootChanged,
                    raw_os_error: None,
                });
            }
            Ok(Self {
                file,
                identity,
                display_path,
                revoked: AtomicBool::new(false),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path;
            Err(ResourceError {
                kind: ResourceErrorKind::UnsupportedPlatform,
                raw_os_error: None,
            })
        }
    }

    pub fn identity(&self) -> ResourceResult<RootIdentity> {
        if self.revoked.load(Ordering::Acquire) {
            return Err(ResourceError {
                kind: ResourceErrorKind::Revoked,
                raw_os_error: None,
            });
        }
        #[cfg(target_os = "macos")]
        {
            let current = macos::open_directory(&self.display_path)?;
            if macos::identity(&self.file)? != self.identity
                || macos::identity(&current)? != self.identity
            {
                return Err(ResourceError {
                    kind: ResourceErrorKind::RootChanged,
                    raw_os_error: None,
                });
            }
            if self.revoked.load(Ordering::Acquire) {
                return Err(ResourceError {
                    kind: ResourceErrorKind::Revoked,
                    raw_os_error: None,
                });
            }
            Ok(self.identity.clone())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(ResourceError {
                kind: ResourceErrorKind::UnsupportedPlatform,
                raw_os_error: None,
            })
        }
    }

    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    pub fn strict_io_available(&self) -> bool {
        false
    }

    pub fn read(&self, _path: &RelativePath, _max_bytes: usize) -> ResourceResult<Vec<u8>> {
        self.identity()?;
        Err(ResourceError {
            kind: ResourceErrorKind::StrictContainmentUnavailable,
            raw_os_error: None,
        })
    }

    pub fn create_new(&self, _path: &RelativePath, _content: &[u8]) -> ResourceResult<()> {
        self.identity()?;
        Err(ResourceError {
            kind: ResourceErrorKind::StrictContainmentUnavailable,
            raw_os_error: None,
        })
    }

    pub fn relation(&self, other: &Self) -> ResourceResult<RootRelation> {
        #[cfg(target_os = "macos")]
        {
            self.identity()?;
            other.identity()?;
            if self.identity == other.identity {
                return Ok(RootRelation::Equivalent);
            }
            let ancestors = macos::ancestry(&self.file)?;
            let other_ancestors = macos::ancestry(&other.file)?;
            if ancestors != macos::ancestry(&self.file)?
                || other_ancestors != macos::ancestry(&other.file)?
            {
                return Ok(RootRelation::Indeterminate);
            }
            self.identity()?;
            other.identity()?;
            if other_ancestors.contains(&self.identity) {
                return Ok(RootRelation::Ancestor);
            }
            if ancestors.contains(&other.identity) {
                return Ok(RootRelation::Descendant);
            }
            Ok(RootRelation::Disjoint)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = other;
            Err(ResourceError {
                kind: ResourceErrorKind::UnsupportedPlatform,
                raw_os_error: None,
            })
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
