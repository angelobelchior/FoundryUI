use std::ffi::{CString, OsStr};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem::{MaybeUninit, size_of};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::VersionedRead;
use super::{
    DirectoryEntry, DirectoryEntryKind, ResourceError, ResourceErrorKind, ResourceResult,
    RootIdentity,
};

const DT_DIR: u8 = 4;
const DT_REG: u8 = 8;
const DT_LNK: u8 = 10;

#[repr(C)]
struct VolumeIdentity {
    length: u32,
    uuid: [u8; 16],
}

pub(super) fn open_directory(path: &Path) -> ResourceResult<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW_ANY | libc::O_CLOEXEC)
        .open(path)
        .map_err(io_error)
}

pub(super) fn identity(file: &File) -> ResourceResult<RootIdentity> {
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(file.as_raw_fd(), metadata.as_mut_ptr()) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    let metadata = unsafe { metadata.assume_init() };
    if metadata.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(ResourceError {
            kind: ResourceErrorKind::NotDirectory,
            raw_os_error: None,
        });
    }
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_UUID,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut filesystem = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), filesystem.as_mut_ptr()) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    let filesystem = unsafe { filesystem.assume_init() };
    let mount_path: Vec<u8> = filesystem
        .f_mntonname
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(|byte| byte as u8)
        .collect();
    let mount = open_directory(Path::new(OsStr::from_bytes(&mount_path)))?;
    let mut mount_stat = MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(mount.as_raw_fd(), mount_stat.as_mut_ptr()) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    if unsafe { mount_stat.assume_init() }.st_dev != metadata.st_dev {
        return Err(ResourceError {
            kind: ResourceErrorKind::Unavailable,
            raw_os_error: None,
        });
    }
    let mut volume = VolumeIdentity {
        length: 0,
        uuid: [0; 16],
    };
    if unsafe {
        libc::fgetattrlist(
            mount.as_raw_fd(),
            (&mut attributes as *mut libc::attrlist).cast(),
            (&mut volume as *mut VolumeIdentity).cast(),
            size_of::<VolumeIdentity>(),
            0,
        )
    } != 0
    {
        return Err(io_error(io::Error::last_os_error()));
    }
    if volume.length as usize != size_of::<VolumeIdentity>() || volume.uuid == [0; 16] {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedVolume,
            raw_os_error: None,
        });
    }
    Ok(RootIdentity {
        volume_uuid: volume.uuid,
        file_id: metadata.st_ino,
        generation: metadata.st_gen,
        birth_seconds: metadata.st_birthtime,
        birth_nanoseconds: metadata.st_birthtime_nsec,
    })
}

pub(super) fn io_error(error: io::Error) -> ResourceError {
    let kind = match error.raw_os_error() {
        Some(libc::ELOOP | libc::ENOTCAPABLE) => ResourceErrorKind::AccessDenied,
        _ => match error.kind() {
            io::ErrorKind::PermissionDenied => ResourceErrorKind::AccessDenied,
            io::ErrorKind::NotADirectory => ResourceErrorKind::NotDirectory,
            io::ErrorKind::Unsupported => ResourceErrorKind::UnsupportedVolume,
            io::ErrorKind::AlreadyExists => ResourceErrorKind::AlreadyExists,
            _ => ResourceErrorKind::Unavailable,
        },
    };
    ResourceError {
        kind,
        raw_os_error: error.raw_os_error(),
    }
}

const O_RESOLVE_BENEATH: libc::c_int = 0x00001000;
const MAX_OPERATION_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn verify_operation_lookup(root: &File) -> ResourceResult<()> {
    let mut filesystem = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(root.as_raw_fd(), filesystem.as_mut_ptr()) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    if unsafe { filesystem.assume_init() }.f_flags & libc::MNT_LOCAL as u32 == 0 {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedVolume,
            raw_os_error: None,
        });
    }
    for path in [c"/", c".."] {
        let descriptor = unsafe {
            libc::openat(
                root.as_raw_fd(),
                path.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW_ANY
                    | O_RESOLVE_BENEATH,
            )
        };
        if descriptor >= 0 {
            drop(unsafe { File::from_raw_fd(descriptor) });
            return Err(ResourceError {
                kind: ResourceErrorKind::OperationContainmentUnavailable,
                raw_os_error: None,
            });
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOTCAPABLE) {
            return Err(ResourceError {
                kind: ResourceErrorKind::OperationContainmentUnavailable,
                raw_os_error: error.raw_os_error(),
            });
        }
    }
    Ok(())
}

fn open_beneath(root: &File, path: &Path, flags: i32, mode: libc::mode_t) -> ResourceResult<File> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ResourceError {
        kind: ResourceErrorKind::InvalidPath,
        raw_os_error: None,
    })?;
    let descriptor = unsafe {
        libc::openat(
            root.as_raw_fd(),
            path.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW_ANY | O_RESOLVE_BENEATH,
            mode as libc::c_uint,
        )
    };
    if descriptor < 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn stat(file: &File) -> ResourceResult<libc::stat> {
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(file.as_raw_fd(), metadata.as_mut_ptr()) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    Ok(unsafe { metadata.assume_init() })
}

fn validate_regular(root: &File, file: &File) -> ResourceResult<()> {
    let actual = stat(file)?;
    if actual.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedResourceType,
            raw_os_error: None,
        });
    }
    if actual.st_dev != stat(root)?.st_dev {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedVolume,
            raw_os_error: None,
        });
    }
    Ok(())
}

pub(super) fn open_regular_read(root: &File, path: &Path) -> ResourceResult<File> {
    let file = open_beneath(root, path, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
    validate_regular(root, &file)?;
    Ok(file)
}

pub(super) fn read_bounded(file: &mut File, max_bytes: usize) -> ResourceResult<Vec<u8>> {
    if max_bytes > MAX_OPERATION_BYTES {
        return Err(ResourceError {
            kind: ResourceErrorKind::LimitExceeded,
            raw_os_error: None,
        });
    }
    let before = stat(file)?;
    if before.st_size < 0 || before.st_size as u64 > max_bytes as u64 {
        return Err(ResourceError {
            kind: ResourceErrorKind::LimitExceeded,
            raw_os_error: None,
        });
    }
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > max_bytes {
        return Err(ResourceError {
            kind: ResourceErrorKind::LimitExceeded,
            raw_os_error: None,
        });
    }
    let after = stat(file)?;
    if before.st_ino != after.st_ino
        || before.st_dev != after.st_dev
        || before.st_size != after.st_size
        || before.st_mtime != after.st_mtime
        || before.st_mtime_nsec != after.st_mtime_nsec
    {
        return Err(ResourceError {
            kind: ResourceErrorKind::ResourceChanged,
            raw_os_error: None,
        });
    }
    Ok(bytes)
}

fn file_identity(metadata: &libc::stat) -> String {
    format!(
        "darwin:{:x}:{:x}:{:x}:{:x}:{:x}",
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_gen,
        metadata.st_birthtime,
        metadata.st_birthtime_nsec
    )
}

fn file_version(metadata: &libc::stat) -> String {
    format!(
        "{}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
        file_identity(metadata),
        metadata.st_size,
        metadata.st_mtime,
        metadata.st_mtime_nsec,
        metadata.st_ctime,
        metadata.st_ctime_nsec,
        metadata.st_mode
    )
}

pub(super) fn read_versioned(file: &mut File, max_bytes: usize) -> ResourceResult<VersionedRead> {
    let before = stat(file)?;
    let bytes = read_bounded(file, max_bytes)?;
    let after = stat(file)?;
    if file_version(&before) != file_version(&after) {
        return Err(ResourceError {
            kind: ResourceErrorKind::ResourceChanged,
            raw_os_error: None,
        });
    }
    Ok(VersionedRead {
        bytes,
        identity_key: file_identity(&after),
        version_key: file_version(&after),
    })
}

pub(super) fn create_new(root: &File, path: &Path, content: &[u8]) -> ResourceResult<()> {
    if content.len() > MAX_OPERATION_BYTES {
        return Err(ResourceError {
            kind: ResourceErrorKind::LimitExceeded,
            raw_os_error: None,
        });
    }
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or(ResourceError {
        kind: ResourceErrorKind::InvalidPath,
        raw_os_error: None,
    })?;
    let parent = open_beneath(root, parent_path, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    if stat(&parent)?.st_dev != stat(root)?.st_dev {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedVolume,
            raw_os_error: None,
        });
    }
    let mut file = open_beneath(
        &parent,
        Path::new(name),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NONBLOCK,
        0o600,
    )?;
    validate_regular(&parent, &file).map_err(|error| ResourceError {
        kind: ResourceErrorKind::WriteIncomplete,
        raw_os_error: error.raw_os_error,
    })?;
    if let Err(error) = file.write_all(content).and_then(|_| file.sync_all()) {
        return Err(ResourceError {
            kind: ResourceErrorKind::WriteIncomplete,
            raw_os_error: error.raw_os_error(),
        });
    }
    Ok(())
}

pub(super) fn rewrite(root: &File, path: &Path, content: &[u8]) -> ResourceResult<()> {
    if content.len() > MAX_OPERATION_BYTES {
        return Err(ResourceError {
            kind: ResourceErrorKind::LimitExceeded,
            raw_os_error: None,
        });
    }
    let (parent_path, name) = parent_and_name(path)?;
    let parent = open_beneath(root, &parent_path, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    if stat(&parent)?.st_dev != stat(root)?.st_dev {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedVolume,
            raw_os_error: None,
        });
    }
    let mut file = open_beneath(
        &parent,
        Path::new(&name),
        libc::O_WRONLY | libc::O_NONBLOCK,
        0,
    )?;
    validate_regular(&parent, &file)?;
    if stat(&file)?.st_nlink > 1 {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedResourceType,
            raw_os_error: None,
        });
    }
    file.set_len(0).map_err(io_error)?;
    if let Err(error) = file.write_all(content).and_then(|_| file.sync_all()) {
        return Err(ResourceError {
            kind: ResourceErrorKind::WriteIncomplete,
            raw_os_error: error.raw_os_error(),
        });
    }
    Ok(())
}

pub(super) fn remove(root: &File, path: &Path) -> ResourceResult<()> {
    let (parent_path, name) = parent_and_name(path)?;
    let parent = open_beneath(root, &parent_path, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    if stat(&parent)?.st_dev != stat(root)?.st_dev {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedVolume,
            raw_os_error: None,
        });
    }
    let target = open_beneath(
        &parent,
        Path::new(&name),
        libc::O_RDONLY | libc::O_NONBLOCK,
        0,
    )?;
    validate_regular(&parent, &target)?;
    let name = CString::new(name.as_bytes()).map_err(|_| ResourceError {
        kind: ResourceErrorKind::InvalidPath,
        raw_os_error: None,
    })?;
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io_error(io::Error::last_os_error()));
    }
    parent.sync_all().map_err(io_error)?;
    Ok(())
}

fn parent_and_name(path: &Path) -> ResourceResult<(std::path::PathBuf, String)> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or(ResourceError {
            kind: ResourceErrorKind::InvalidPath,
            raw_os_error: None,
        })?;
    Ok((parent.to_path_buf(), name.to_string()))
}

pub(super) fn list_directory(root: &File, path: &Path) -> ResourceResult<Vec<DirectoryEntry>> {
    let dir = open_beneath(root, path, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    if stat(&dir)?.st_dev != stat(root)?.st_dev {
        return Err(ResourceError {
            kind: ResourceErrorKind::UnsupportedVolume,
            raw_os_error: None,
        });
    }
    let raw = dir.into_raw_fd();
    let stream = unsafe { libc::fdopendir(raw) };
    if stream.is_null() {
        let error = io::Error::last_os_error();
        unsafe { libc::close(raw) };
        return Err(io_error(error));
    }
    let mut entries = Vec::new();
    loop {
        unsafe { *libc::__error() = 0 };
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let error_number = unsafe { *libc::__error() };
            if error_number != 0 {
                let error = io::Error::from_raw_os_error(error_number);
                unsafe { libc::closedir(stream) };
                return Err(io_error(error));
            }
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        let name = name.to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        let kind = match unsafe { (*entry).d_type } {
            DT_DIR => DirectoryEntryKind::Directory,
            DT_REG => DirectoryEntryKind::File,
            DT_LNK => DirectoryEntryKind::Symlink,
            _ => DirectoryEntryKind::Other,
        };
        entries.push(DirectoryEntry { name, kind });
    }
    unsafe { libc::closedir(stream) };
    Ok(entries)
}

pub(super) fn ancestry(file: &File) -> ResourceResult<Vec<RootIdentity>> {
    let mut current = file.try_clone().map_err(io_error)?;
    let mut ancestors = Vec::new();
    for _ in 0..256 {
        let current_identity = identity(&current)?;
        if ancestors.contains(&current_identity) {
            return Err(ResourceError {
                kind: ResourceErrorKind::Unavailable,
                raw_os_error: None,
            });
        }
        ancestors.push(current_identity.clone());
        let descriptor = unsafe {
            libc::openat(
                current.as_raw_fd(),
                c"..".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW_ANY | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(io_error(io::Error::last_os_error()));
        }
        let parent = unsafe { File::from_raw_fd(descriptor) };
        if identity(&parent)? == current_identity {
            return Ok(ancestors);
        }
        current = parent;
    }
    Err(ResourceError {
        kind: ResourceErrorKind::Unavailable,
        raw_os_error: None,
    })
}
