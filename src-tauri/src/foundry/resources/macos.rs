use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io;
use std::mem::{MaybeUninit, size_of};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::{ResourceError, ResourceErrorKind, ResourceResult, RootIdentity};

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
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => ResourceErrorKind::AccessDenied,
        io::ErrorKind::NotADirectory => ResourceErrorKind::NotDirectory,
        io::ErrorKind::Unsupported => ResourceErrorKind::UnsupportedVolume,
        _ => ResourceErrorKind::Unavailable,
    };
    ResourceError {
        kind,
        raw_os_error: error.raw_os_error(),
    }
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
