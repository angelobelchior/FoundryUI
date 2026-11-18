use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

#[test]
fn selected_aliases_resolve_to_the_same_live_root_identity() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    std::fs::create_dir(&selected).expect("directory");
    let alias = folder.path().join("alias");
    symlink(&selected, &alias).expect("alias");
    let root = RootHandle::enroll(&selected).expect("root");
    let same = RootHandle::enroll(&alias).expect("alias root");
    assert_eq!(
        root.identity().expect("identity"),
        same.identity().expect("identity")
    );
    assert_eq!(
        root.relation(&same).expect("relation"),
        RootRelation::Equivalent
    );
    assert_eq!(
        root.display_path(),
        std::fs::canonicalize(selected).expect("canonical path")
    );
    assert!(!root.identity().expect("identity").storage_key().is_empty());
}

#[test]
fn root_overlap_uses_opened_directory_ancestry_instead_of_string_prefixes() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let parent = folder.path().join("project");
    let child = parent.join("child");
    let sibling = folder.path().join("project-other");
    std::fs::create_dir_all(&child).expect("child");
    std::fs::create_dir(&sibling).expect("sibling");
    let parent = RootHandle::enroll(&parent).expect("parent root");
    let child = RootHandle::enroll(&child).expect("child root");
    let sibling = RootHandle::enroll(&sibling).expect("sibling root");
    assert_eq!(
        parent.relation(&child).expect("relation"),
        RootRelation::Ancestor
    );
    assert_eq!(
        child.relation(&parent).expect("relation"),
        RootRelation::Descendant
    );
    assert_eq!(
        parent.relation(&sibling).expect("relation"),
        RootRelation::Disjoint
    );
}

#[test]
fn root_replacement_and_revocation_never_authorize_a_new_target() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let path = folder.path().join("selected");
    std::fs::create_dir(&path).expect("selected");
    let root = RootHandle::enroll(&path).expect("root");
    std::fs::rename(&path, folder.path().join("moved")).expect("move root");
    std::fs::create_dir(&path).expect("replacement");
    assert_eq!(
        root.identity().expect_err("replaced root").kind,
        ResourceErrorKind::RootChanged
    );
    let replacement = RootHandle::enroll(&path).expect("replacement root");
    replacement.revoke();
    assert_eq!(
        replacement.identity().expect_err("revoked root").kind,
        ResourceErrorKind::Revoked
    );
}

#[test]
fn operation_scoped_io_reads_and_creates_without_claiming_continuous_location_containment() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    std::fs::create_dir(&selected).expect("directory");
    std::fs::write(selected.join("input"), "fixture-content").expect("fixture");
    let root = RootHandle::enroll(&selected).expect("root");
    let input = RelativePath::parse("input").expect("relative path");
    let new_file = RelativePath::parse("new-file").expect("relative path");
    assert!(root.operation_io_available().expect("operation capability"));
    assert_eq!(root.read(&input, 4096).expect("read"), b"fixture-content");
    root.create_new(&new_file, b"new-content").expect("create");
    assert_eq!(
        std::fs::read(selected.join("new-file")).expect("created file"),
        b"new-content"
    );
    assert_eq!(
        std::fs::read_to_string(selected.join("input")).expect("unchanged fixture"),
        "fixture-content"
    );
    for path in [
        "",
        ".",
        "../outside",
        "child/../../outside",
        "/absolute",
        "C:\\outside",
        "\\\\server\\share",
        "bad\0path",
    ] {
        assert_eq!(
            RelativePath::parse(path).expect_err("invalid path").kind,
            ResourceErrorKind::InvalidPath
        );
    }
}

#[test]
fn changing_a_selected_alias_does_not_change_its_enrolled_target() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let first = folder.path().join("first");
    let second = folder.path().join("second");
    let alias = folder.path().join("alias");
    std::fs::create_dir(&first).expect("first");
    std::fs::create_dir(&second).expect("second");
    symlink(&first, &alias).expect("alias");
    let original = RootHandle::enroll(&alias).expect("original root");
    std::fs::remove_file(&alias).expect("remove alias");
    symlink(&second, &alias).expect("retarget alias");
    assert_eq!(
        original.identity().expect("original identity"),
        RootHandle::enroll(&first)
            .expect("first root")
            .identity()
            .expect("identity")
    );
    assert_ne!(
        original.identity().expect("original identity"),
        RootHandle::enroll(&alias)
            .expect("new root")
            .identity()
            .expect("identity")
    );
}

#[test]
fn filesystem_case_rules_are_observed_without_lowercasing_identity_keys() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("MiXeD");
    let lower = folder.path().join("mixed");
    std::fs::create_dir(&selected).expect("directory");
    let root = RootHandle::enroll(&selected).expect("root");
    if lower.exists() {
        assert_eq!(
            root.relation(&RootHandle::enroll(&lower).expect("case alias"))
                .expect("relation"),
            RootRelation::Equivalent
        );
    } else {
        assert_eq!(
            RootHandle::enroll(&lower)
                .err()
                .expect("missing alias")
                .kind,
            ResourceErrorKind::Unavailable
        );
    }
}

#[test]
fn missing_and_denied_roots_are_errors_not_empty_authorized_results() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    std::fs::create_dir(&selected).expect("directory");
    let root = RootHandle::enroll(&selected).expect("root");
    let permissions = std::fs::metadata(&selected)
        .expect("metadata")
        .permissions();
    std::fs::set_permissions(&selected, std::fs::Permissions::from_mode(0)).expect("deny");
    let denied = root.identity();
    std::fs::set_permissions(&selected, permissions).expect("restore");
    assert_eq!(
        denied.expect_err("denied").kind,
        ResourceErrorKind::AccessDenied
    );
    std::fs::remove_dir(&selected).expect("remove");
    let unavailable = root
        .read(&RelativePath::parse("input").expect("path"), 1024)
        .expect_err("unavailable");
    assert_eq!(unavailable.kind, ResourceErrorKind::Unavailable);
    assert!(unavailable.raw_os_error.is_some());
}

#[test]
fn a_parent_moved_before_an_operation_is_not_reopened_outside_the_root() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    let child = selected.join("child");
    let moved = folder.path().join("moved");
    std::fs::create_dir_all(&child).expect("child");
    let root = RootHandle::enroll(&selected).expect("root");
    let child_root = RootHandle::enroll(&child).expect("child root");
    std::fs::rename(&child, &moved).expect("move parent");
    let error = root
        .create_new(
            &RelativePath::parse("child/new-file").expect("path"),
            b"not authorized",
        )
        .expect_err("blocked creation");
    assert_eq!(error.kind, ResourceErrorKind::Unavailable);
    assert!(!moved.join("new-file").exists());
    assert!(root.relation(&child_root).is_err());
}

#[test]
fn an_opened_operation_keeps_its_original_object_after_movement_and_path_replacement() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    std::fs::create_dir(&selected).expect("directory");
    std::fs::write(selected.join("input"), b"original-object").expect("fixture");
    let root = RootHandle::enroll(&selected).expect("root");
    let path = RelativePath::parse("input").expect("path");
    let operation = root.open_read(&path).expect("validated operation");
    std::fs::rename(selected.join("input"), folder.path().join("moved-input")).expect("move");
    std::fs::write(selected.join("input"), b"replacement-object").expect("replacement");
    assert_eq!(
        operation.read(128).expect("same opened object"),
        b"original-object"
    );
    assert_eq!(
        root.read(&path, 128).expect("new operation"),
        b"replacement-object"
    );
}

#[test]
fn child_symlinks_never_allow_reading_or_creating_through_another_target() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    let outside = folder.path().join("outside");
    std::fs::create_dir(&selected).expect("selected");
    std::fs::create_dir(&outside).expect("outside");
    std::fs::write(outside.join("secret"), b"outside-data").expect("outside fixture");
    std::fs::write(selected.join("inside"), b"inside-data").expect("inside fixture");
    symlink(outside.join("secret"), selected.join("link")).expect("file link");
    symlink(&outside, selected.join("directory-link")).expect("directory link");
    symlink(selected.join("inside"), selected.join("internal-link")).expect("internal link");
    let root = RootHandle::enroll(&selected).expect("root");
    for path in ["link", "directory-link/secret", "internal-link"] {
        assert_eq!(
            root.read(&RelativePath::parse(path).expect("path"), 128)
                .expect_err("link denied")
                .kind,
            ResourceErrorKind::AccessDenied
        );
    }
    assert!(
        root.create_new(
            &RelativePath::parse("directory-link/new").expect("path"),
            b"not allowed"
        )
        .is_err()
    );
    assert!(!outside.join("new").exists());
    assert!(
        root.create_new(&RelativePath::parse("link").expect("path"), b"not allowed")
            .is_err()
    );
    assert_eq!(
        std::fs::read(outside.join("secret")).expect("unchanged outside"),
        b"outside-data"
    );
}

#[test]
fn exclusive_creation_preserves_existing_files_and_uses_the_validated_parent() {
    let folder = tempfile::tempdir().expect("temporary directory");
    std::fs::create_dir(folder.path().join("parent")).expect("parent");
    std::fs::write(folder.path().join("existing"), b"keep-me").expect("fixture");
    let root = RootHandle::enroll(folder.path()).expect("root");
    assert_eq!(
        root.create_new(
            &RelativePath::parse("existing").expect("path"),
            b"overwrite"
        )
        .expect_err("existing")
        .kind,
        ResourceErrorKind::AlreadyExists
    );
    root.create_new(
        &RelativePath::parse("parent/new").expect("path"),
        b"created",
    )
    .expect("nested creation");
    assert_eq!(
        root.read(&RelativePath::parse("parent/new").expect("path"), 128)
            .expect("read created"),
        b"created"
    );
    assert_eq!(
        std::fs::read(folder.path().join("existing")).expect("unchanged"),
        b"keep-me"
    );
    let permissions = std::fs::metadata(folder.path().join("parent/new"))
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(permissions & 0o777, 0o600);
}

#[test]
fn bounded_reads_distinguish_empty_missing_oversized_and_special_resources() {
    let folder = tempfile::tempdir().expect("temporary directory");
    std::fs::write(folder.path().join("empty"), []).expect("empty");
    std::fs::write(folder.path().join("large"), b"12345").expect("large");
    std::fs::create_dir(folder.path().join("directory")).expect("directory");
    let fifo = std::ffi::CString::new(folder.path().join("fifo").as_os_str().as_encoded_bytes())
        .expect("FIFO path");
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let root = RootHandle::enroll(folder.path()).expect("root");
    assert_eq!(
        root.read(&RelativePath::parse("empty").expect("path"), 0)
            .expect("empty EOF"),
        b""
    );
    assert_eq!(
        root.read(&RelativePath::parse("missing").expect("path"), 16)
            .expect_err("missing")
            .kind,
        ResourceErrorKind::Unavailable
    );
    assert_eq!(
        root.read(&RelativePath::parse("large").expect("path"), 4)
            .expect_err("too large")
            .kind,
        ResourceErrorKind::LimitExceeded
    );
    for path in ["directory", "fifo"] {
        assert_eq!(
            root.read(&RelativePath::parse(path).expect("path"), 16)
                .expect_err("special resource")
                .kind,
            ResourceErrorKind::UnsupportedResourceType
        );
    }
}

#[test]
fn explicit_revocation_stops_prepared_reads_and_new_effects() {
    let folder = tempfile::tempdir().expect("temporary directory");
    std::fs::write(folder.path().join("input"), b"private-data").expect("fixture");
    let root = RootHandle::enroll(folder.path()).expect("root");
    let operation = root
        .open_read(&RelativePath::parse("input").expect("path"))
        .expect("open");
    root.revoke();
    assert_eq!(
        operation.read(128).expect_err("revoked operation").kind,
        ResourceErrorKind::Revoked
    );
    assert_eq!(
        root.create_new(&RelativePath::parse("new").expect("path"), b"not allowed")
            .expect_err("revoked creation")
            .kind,
        ResourceErrorKind::Revoked
    );
    assert!(!folder.path().join("new").exists());
}

#[test]
fn post_create_validation_failure_reports_a_partial_effect_without_unlinking() {
    if std::env::var_os("FOUNDRY_IO_FAULT_CHILD").is_some() {
        let folder = tempfile::tempdir().expect("temporary directory");
        let root = RootHandle::enroll(folder.path()).expect("root");
        let error = root
            .create_new(
                &RelativePath::parse("post-create-validation").expect("path"),
                b"not written",
            )
            .expect_err("injected validation failure");
        assert_eq!(error.kind, ResourceErrorKind::WriteIncomplete);
        assert_eq!(error.raw_os_error, Some(libc::EIO));
        let metadata = std::fs::metadata(folder.path().join("post-create-validation"))
            .expect("created entry remains");
        assert_eq!(metadata.len(), 0);
        return;
    }
    let fixture = tempfile::tempdir().expect("fixture directory");
    let library = fixture.path().join("fail_created_stat.dylib");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/foundry/resources/test-fixtures/fail_created_stat.c");
    let compiled = std::process::Command::new("/usr/bin/cc")
        .args(["-dynamiclib", "-Wall", "-Wextra", "-Werror"])
        .arg(source)
        .arg("-o")
        .arg(&library)
        .output()
        .expect("compile local fault fixture");
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let child = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "foundry::resources::tests::post_create_validation_failure_reports_a_partial_effect_without_unlinking", "--nocapture"])
        .env("FOUNDRY_IO_FAULT_CHILD", "1").env("DYLD_INSERT_LIBRARIES", &library)
        .output().expect("run isolated fault fixture");
    assert!(
        child.status.success(),
        "{}\n{}\n{}",
        child.status,
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}
