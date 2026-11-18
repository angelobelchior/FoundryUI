use std::ffi::CStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::mem::{MaybeUninit, size_of};
use std::net::{IpAddr, SocketAddr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{LocalInferenceError, RuntimeObservation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeIdentity {
    pid: u32,
    started_seconds: u64,
    started_micros: u64,
    executable: PathBuf,
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

pub(crate) fn verify(
    observation: &RuntimeObservation,
) -> Result<RuntimeIdentity, LocalInferenceError> {
    let before = identity(observation)?;
    listener_owner(observation)?;
    let after = identity(observation)?;
    if before != after {
        return Err(LocalInferenceError::SessionChanged);
    }
    Ok(after)
}

pub(crate) fn verify_connection(
    observation: &RuntimeObservation,
    client_address: SocketAddr,
) -> Result<Option<RuntimeIdentity>, LocalInferenceError> {
    let before = identity(observation)?;
    let text = socket_table(observation.address.port(), "ESTABLISHED")?;
    let mut pid = None;
    let mut found = false;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = Some(
                value
                    .parse::<u32>()
                    .map_err(|_| LocalInferenceError::ListenerUnverified)?,
            );
        } else if let Some(name) = line.strip_prefix('n') {
            let (server, client) = name
                .split_once("->")
                .ok_or(LocalInferenceError::ListenerUnverified)?;
            let server: SocketAddr = server
                .parse()
                .map_err(|_| LocalInferenceError::ListenerUnverified)?;
            let client: SocketAddr = client
                .parse()
                .map_err(|_| LocalInferenceError::ListenerUnverified)?;
            if server == observation.address && client == client_address {
                if pid != Some(observation.pid) {
                    return Err(LocalInferenceError::ListenerUnverified);
                }
                found = true;
            }
        }
    }
    let after = identity(observation)?;
    if before != after {
        return Err(LocalInferenceError::SessionChanged);
    }
    Ok(found.then_some(after))
}

fn identity(observation: &RuntimeObservation) -> Result<RuntimeIdentity, LocalInferenceError> {
    let invalid = || LocalInferenceError::RuntimeUnverified;
    let pid: libc::pid_t = observation.pid.try_into().map_err(|_| invalid())?;
    if pid <= 0 || !observation.expected_executable.is_absolute() {
        return Err(invalid());
    }
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let actual_size = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    if actual_size != size_of::<libc::proc_bsdinfo>() as i32 {
        return Err(invalid());
    }
    let info = unsafe { info.assume_init() };
    let mut buffer = vec![0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let length =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return Err(invalid());
    }
    let path = CStr::from_bytes_until_nul(&buffer).map_err(|_| invalid())?;
    let executable = PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes()))
        .canonicalize()
        .map_err(|_| invalid())?;
    let expected = observation
        .expected_executable
        .canonicalize()
        .map_err(|_| invalid())?;
    if executable != expected || info.pbi_pid != observation.pid {
        return Err(invalid());
    }
    let metadata = std::fs::metadata(&executable).map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.mode() & 0o111 == 0 {
        return Err(invalid());
    }
    Ok(RuntimeIdentity {
        pid: observation.pid,
        started_seconds: info.pbi_start_tvsec,
        started_micros: info.pbi_start_tvusec,
        executable,
        device: metadata.dev(),
        inode: metadata.ino(),
        size: metadata.size(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    })
}

fn listener_owner(observation: &RuntimeObservation) -> Result<(), LocalInferenceError> {
    let invalid = || LocalInferenceError::ListenerUnverified;
    let text = socket_table(observation.address.port(), "LISTEN")?;
    let mut pid = None;
    let mut found = false;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = Some(value.parse::<u32>().map_err(|_| invalid())?);
        } else if let Some(name) = line.strip_prefix('n') {
            let (host, port) = name.rsplit_once(':').ok_or_else(invalid)?;
            let port = port.parse::<u16>().map_err(|_| invalid())?;
            let host = host.trim_start_matches('[').trim_end_matches(']');
            let matching = if host == "*" {
                port == observation.address.port()
            } else {
                let ip: IpAddr = host.parse().map_err(|_| invalid())?;
                let address = SocketAddr::new(ip, port);
                address == observation.address
                    || (ip.is_unspecified() && port == observation.address.port())
            };
            if matching {
                if pid != Some(observation.pid) {
                    return Err(invalid());
                }
                found = true;
            }
        }
    }
    if !found {
        return Err(invalid());
    }
    Ok(())
}

fn socket_table(port: u16, state: &str) -> Result<String, LocalInferenceError> {
    let invalid = || LocalInferenceError::ListenerUnverified;
    let mut output = tempfile::tempfile().map_err(|_| invalid())?;
    let mut child = Command::new("/usr/sbin/lsof")
        .args([
            "-nP",
            "-a",
            &format!("-iTCP:{port}"),
            &format!("-sTCP:{state}"),
            "-Fpn",
        ])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone().map_err(|_| invalid())?))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| invalid())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(invalid());
            }
        }
    };
    if !matches!(status.code(), Some(0 | 1))
        || output.metadata().map_err(|_| invalid())?.len() > 65_536
    {
        return Err(invalid());
    }
    read_output(&mut output).map_err(|_| invalid())
}

fn read_output(file: &mut File) -> std::io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.take(65_537).read_to_string(&mut text)?;
    Ok(text)
}
