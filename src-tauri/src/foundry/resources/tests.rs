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
fn strict_io_stays_unavailable_instead_of_downgrading_to_pinned_object_access() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    std::fs::create_dir(&selected).expect("directory");
    std::fs::write(selected.join("input"), "fixture-content").expect("fixture");
    let root = RootHandle::enroll(&selected).expect("root");
    let input = RelativePath::parse("input").expect("relative path");
    let new_file = RelativePath::parse("new-file").expect("relative path");
    assert!(!root.strict_io_available());
    assert_eq!(
        root.read(&input, 4096)
            .expect_err("strict containment unavailable")
            .kind,
        ResourceErrorKind::StrictContainmentUnavailable
    );
    assert_eq!(
        root.create_new(&new_file, b"must-not-write")
            .expect_err("strict containment unavailable")
            .kind,
        ResourceErrorKind::StrictContainmentUnavailable
    );
    assert!(!selected.join("new-file").exists());
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
fn a_moved_parent_cannot_turn_strict_creation_into_a_write_outside_the_root() {
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
    assert_eq!(error.kind, ResourceErrorKind::StrictContainmentUnavailable);
    assert!(!moved.join("new-file").exists());
    assert!(root.relation(&child_root).is_err());
}
