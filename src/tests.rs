use super::*;
use std::process::Command;

fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit_at(dir: &std::path::Path, date: &str) -> String {
    let output = Command::new("git")
        .args(["commit", "--allow-empty", "-m", "test"])
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_in(dir, &["rev-parse", "HEAD"])
}

fn merge_at(dir: &std::path::Path, other: &str, date: &str, message: &str) -> String {
    let output = Command::new("git")
        .args(["merge", "--no-ff", "-m", message, other])
        .current_dir(dir)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_in(dir, &["rev-parse", "HEAD"])
}

fn new_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git_in(dir.path(), &["init", "-b", "main"]);
    dir
}

fn delete_object(dir: &std::path::Path, hash: &str) {
    let obj_path = dir.join(".git/objects").join(&hash[..2]).join(&hash[2..]);
    std::fs::remove_file(obj_path).unwrap();
}

/// Git's "local" clone transport optimization ignores `--depth`/`--filter`
/// entirely (with a warning) and copies the whole object store; a `file://`
/// URL forces the smart-HTTP-like transport that actually honors them, so
/// shallow/partial-clone fixtures need this to be genuine.
fn file_url(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

// Pure function tests

#[test]
fn is_gregorian_date_valid() {
    assert!(is_gregorian_date("19700101"));
    assert!(is_gregorian_date("19691231"));
    assert!(is_gregorian_date("20260412"));
    assert!(is_gregorian_date("20001231"));
    assert!(is_gregorian_date("20240229"));
    assert!(is_gregorian_date("20000229"));
    assert!(is_gregorian_date("00010101"));
    assert!(is_gregorian_date("99991231"));
}

#[test]
fn is_gregorian_date_invalid() {
    assert!(!is_gregorian_date("20261301"));
    assert!(!is_gregorian_date("20260032"));
    assert!(!is_gregorian_date("20260000"));
    assert!(!is_gregorian_date("20260431"));
    assert!(!is_gregorian_date("20230229"));
    assert!(!is_gregorian_date("19000229"));
    assert!(!is_gregorian_date("00000101"));
}

#[test]
fn epoch_to_date_known_values() {
    assert_eq!(epoch_to_date(0).unwrap(), "19700101");
    assert_eq!(epoch_to_date(1_000_000_000).unwrap(), "20010909");
    assert_eq!(epoch_to_date(86399).unwrap(), "19700101");
    assert_eq!(epoch_to_date(86400).unwrap(), "19700102");
}

#[test]
fn epoch_to_date_out_of_range() {
    assert!(matches!(
        epoch_to_date(i64::MAX).unwrap_err(),
        Error::Git(_)
    ));
}

#[test]
fn reverse_input_bare() {
    assert_eq!(
        reverse_input("20260412.1", "").unwrap(),
        Some(("20260412", 1))
    );
    assert_eq!(
        reverse_input("20260412.42", "").unwrap(),
        Some(("20260412", 42))
    );
}

#[test]
fn reverse_input_prefixed() {
    assert_eq!(
        reverse_input("v0.20260412.3", "v0.").unwrap(),
        Some(("20260412", 3))
    );
    assert_eq!(
        reverse_input("0.20260412.1", "0.").unwrap(),
        Some(("20260412", 1))
    );
}

#[test]
fn reverse_input_requires_the_configured_prefix() {
    let err = reverse_input("20260412.1", "v0.").unwrap_err();
    assert!(matches!(
        &err,
        Error::MissingPrefix { version, prefix } if version == "20260412.1" && prefix == "v0."
    ));
    assert_eq!(
        err.to_string(),
        "version 20260412.1 is missing required prefix \"v0.\""
    );
}

#[test]
fn reverse_input_infers_no_prefix() {
    assert_eq!(reverse_input("v0.20260412.3", "").unwrap(), None);
    assert_eq!(reverse_input("0.20260412.1", "").unwrap(), None);
    assert_eq!(reverse_input("0.20260412.1", "v0.").unwrap(), None);
}

#[test]
fn reverse_input_rejects_non_versions() {
    for target in [
        "",
        "notaversion",
        "20260412.0",
        "20260412.01",
        "20260412",
        "1234567.1",
        "20260412.abc",
        "20260412.1.2",
        "release-20260412.2",
        "20260412.5-dirty.abc1234",
        "20260412.1\n",
        "20260412.1\n20260412.2",
        "x\n20260412.1",
        "abcdefgh.1",
        "hotfix-1.2",
        "version1.2",
        "20260412.",
    ] {
        assert_eq!(reverse_input(target, "").unwrap(), None, "{target:?}");
    }
}

#[test]
fn reverse_input_non_ascii_does_not_panic() {
    for target in [
        "日20260412.7",
        "café20260412.1",
        "20260412.1é",
        "1234567日8.1",
        "aééxyz.1",
        "é",
    ] {
        assert_eq!(reverse_input(target, "").unwrap(), None, "{target:?}");
    }
}

#[test]
fn reverse_input_invalid_date_is_still_reverse_mode() {
    for target in ["20261301.1", "20260230.1", "20230229.1", "00000101.1"] {
        let err = reverse_input(target, "").unwrap_err();
        assert!(
            matches!(&err, Error::InvalidVersionDate(v) if v == target),
            "{target:?}: {err}"
        );
    }
    assert_eq!(
        reverse_input("19691231.1", "").unwrap(),
        Some(("19691231", 1))
    );
    assert_eq!(
        reverse_input("20240229.1", "").unwrap(),
        Some(("20240229", 1))
    );
}

#[test]
fn reverse_input_count_overflow() {
    let err = reverse_input("20260412.18446744073709551616", "").unwrap_err();
    assert!(matches!(err, Error::InvalidVersionCount(_)));
    assert_eq!(
        err.to_string(),
        "invalid count in version: 20260412.18446744073709551616"
    );
}

#[test]
fn format_version_clean() {
    assert_eq!(
        format_version("", "20260412", 1, false, "", ""),
        "20260412.1"
    );
    assert_eq!(
        format_version("v0.", "20260412", 3, false, "", ""),
        "v0.20260412.3"
    );
}

#[test]
fn format_version_dirty_with_hash() {
    assert_eq!(
        format_version("", "20260412", 1, true, "-dirty", "abc1234"),
        "20260412.1-dirty.abc1234",
    );
}

#[test]
fn format_version_dirty_no_hash() {
    assert_eq!(
        format_version("", "20260412", 1, true, "-dirty", ""),
        "20260412.1-dirty",
    );
}

#[test]
fn options_default() {
    let opts = Options::default();
    assert_eq!(opts.dir, std::path::Path::new("."));
    assert!(opts.target.is_none());
    assert_eq!(opts.prefix, "");
    assert!(opts.dirty_suffix.is_none());
    assert!(opts.include_dirty_hash);
    assert!(opts.branch.is_none());
    assert!(opts.remote.is_none());
    assert!(!opts.short);
}

// Git-dependent tests: basic forward/reverse

#[test]
fn forward_basic() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn branch_override_not_found() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        branch: Some("nonexistent"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NoDefaultBranch));
}

#[test]
fn no_default_branch() {
    let dir = tempfile::tempdir().unwrap();
    git_in(dir.path(), &["init", "-b", "develop"]);
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NoDefaultBranch));
}

#[test]
fn origin_head_detection() {
    let remote = new_repo();
    commit_at(remote.path(), "2026-04-10T12:00:00Z");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &["clone", remote.path().to_str().unwrap(), "local"],
    );
    let local = parent.path().join("local");
    commit_at(&local, "2026-04-10T13:00:00Z");

    let result = run(&Options {
        dir: &local,
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.2");
}

#[test]
fn corrupt_commit_propagates_error() {
    // The deleted commit is same-date as HEAD, so under the 0.3 cohort walk
    // it is reached as a same-date parent that fails to load: incomplete
    // local history, not a generic git error.
    let dir = new_repo();
    let first = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T13:00:00Z");

    delete_object(dir.path(), &first);

    let err = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn short_hash_returns_first_seven_chars() {
    let id = gix::ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
    assert_eq!(short_hash(id), "deadbee");
}

#[test]
fn short_hash_no_abbreviation_even_with_core_abbrev_set() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["config", "core.abbrev", "12"]);
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    let feature_hash = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    let result = run(&Options {
        dir: dir.path(),
        target: Some("feature"),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    let hash_part = result.rsplit('.').next().unwrap();
    assert_eq!(hash_part.len(), 7);
    assert!(feature_hash.starts_with(hash_part));
}

#[test]
fn forward_auto_detect_branch() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn forward_multiple_same_day() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T13:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.2");
}

#[test]
fn forward_across_days() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-09T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn forward_specific_revision() {
    let dir = new_repo();
    let first = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T13:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some(&first),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn forward_wrong_branch() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    let feature_hash = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    let err = run(&Options {
        dir: dir.path(),
        target: Some(&feature_hash),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NotOnDefaultBranch { .. }));
}

#[test]
fn off_branch_dirty_version() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T13:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    let result = run(&Options {
        dir: dir.path(),
        target: Some("feature"),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    // Version is from the anchor (newest reachable selected-chain commit,
    // here main's tip = 20260410.1), hash is from the feature branch commit.
    assert!(result.starts_with("20260410.1-dirty."));
}

#[test]
fn off_branch_dirty_no_hash() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T13:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    let result = run(&Options {
        dir: dir.path(),
        target: Some("feature"),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        include_dirty_hash: false,
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1-dirty");
}

#[test]
fn dirty_hash_is_head_not_anchor() {
    // The dirty hash always identifies the original target, never the
    // anchor commit used to compute the base version.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    let feature_hash = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    let result = run(&Options {
        dir: dir.path(),
        target: Some("feature"),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    let hash_part = result.rsplit('.').next().unwrap();
    assert!(feature_hash.starts_with(hash_part));
}

#[test]
fn forward_dirty_untracked() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    std::fs::write(dir.path().join("untracked.txt"), "dirty").unwrap();
    let err = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::DirtyWorkspace));
}

#[test]
fn forward_dirty_staged() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    std::fs::write(dir.path().join("staged.txt"), "staged").unwrap();
    git_in(dir.path(), &["add", "staged.txt"]);
    let err = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::DirtyWorkspace));
}

#[test]
fn forward_dirty_with_suffix() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    std::fs::write(dir.path().join("dirty.txt"), "dirty").unwrap();
    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    assert!(result.starts_with("20260410.1-dirty."));
}

#[test]
fn forward_dirty_no_hash() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    std::fs::write(dir.path().join("dirty.txt"), "dirty").unwrap();
    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        include_dirty_hash: false,
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1-dirty");
}

#[test]
fn reverse_basic() {
    let dir = new_repo();
    let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some("20260410.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, hash);
}

#[test]
fn reverse_short() {
    let dir = new_repo();
    let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some("20260410.1"),
        branch: Some("main"),
        short: true,
        ..Options::default()
    })
    .unwrap();
    assert!(hash.starts_with(&result));
}

#[test]
fn reverse_multiple_same_day() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let second = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some("20260410.2"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, second);
}

#[test]
fn zz_scratch_reverse_whole_history_one_date_root_block() {
    // Mirrors sh's whole_history_one_date_root_block: an entire 3-commit
    // history shares one date and ends at a genuine root. Reverse-lookup for
    // N=1 must walk past two same-date members (c3, c2) before reaching the
    // true root (c1) and must correctly resolve to it rather than erroring.
    let dir = new_repo();
    let first = commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    commit_at(dir.path(), "2026-04-10T11:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some("20260410.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, first);
}

#[test]
fn reverse_across_days() {
    let dir = new_repo();
    let first = commit_at(dir.path(), "2026-04-09T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some("20260409.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, first);
}

#[test]
fn reverse_not_found() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        target: Some("20260410.99"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::VersionNotFound(_)));
}

#[test]
fn forward_target_is_branch_tip() {
    let dir = new_repo();
    let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some(&hash),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn decreasing_dates() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-11T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::DecreasingDate { .. }));
}

#[test]
fn reverse_walks_past_date() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-08T12:00:00Z");
    let target = commit_at(dir.path(), "2026-04-09T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some("20260409.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, target);
}

#[test]
fn origin_head_dangling_target_unresolvable() {
    // A dangling `refs/remotes/origin/HEAD` selects the branch NAME it
    // points to, exactly like sh's `detect_default_branch`; it is not
    // silently skipped in favor of a lower-precedence tier. Resolving that
    // name to a tip then fails, since neither a local nor remote-tracking
    // ref by that name exists.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(
        dir.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/nonexistent",
        ],
    );
    let err = run(&Options {
        dir: dir.path(),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NoDefaultBranch));
}

#[test]
fn not_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let err = run(&Options {
        dir: dir.path(),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NotARepository));
}

#[test]
fn empty_repository() {
    let dir = new_repo();
    let err = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::EmptyRepository));
}

#[test]
fn invalid_revision() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        target: Some("nonexistent_ref"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::RevisionNotFound(_)));
}

// --- Target and version input handling ---

fn run_target(dir: &std::path::Path, target: &str) -> Result<String, Error> {
    run(&Options {
        dir,
        target: Some(target),
        branch: Some("main"),
        ..Options::default()
    })
}

fn tag_annotated(dir: &std::path::Path, name: &str, object: &str) {
    git_in(
        dir,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@test.com",
            "tag",
            "--no-sign",
            "-a",
            "-m",
            "annotated",
            name,
            object,
        ],
    );
}

#[test]
fn explicit_empty_target_is_rejected() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run_target(dir.path(), "").unwrap_err();
    assert!(matches!(&err, Error::RevisionNotFound(rev) if rev.is_empty()));
}

#[test]
fn annotated_tag_target_resolves_to_its_commit() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    tag_annotated(dir.path(), "v1", "HEAD");
    tag_annotated(dir.path(), "v1-again", "v1");
    let tag_object = git_in(dir.path(), &["rev-parse", "v1"]);

    for target in ["v1", "v1-again", tag_object.as_str()] {
        assert_eq!(run_target(dir.path(), target).unwrap(), "20260410.2");
    }
}

#[test]
fn tree_target_is_not_a_revision() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run_target(dir.path(), "HEAD^{tree}").unwrap_err();
    assert!(matches!(&err, Error::RevisionNotFound(rev) if rev == "HEAD^{tree}"));
}

#[test]
fn tag_cycle_is_not_a_revision() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let id = "1".repeat(40);
    let tag = format!("object {id}\ntype tag\ntag loop\ntagger T <t@t> 0 +0000\n\nloop\n");
    std::fs::write(dir.path().join("loop.tag"), tag).unwrap();
    let written = git_in(
        dir.path(),
        &["hash-object", "-w", "-t", "tag", "--literally", "loop.tag"],
    );
    // Stored under the ID its own header names, the tag points at itself.
    let objects = dir.path().join(".git/objects");
    std::fs::create_dir_all(objects.join("11")).unwrap();
    std::fs::copy(
        objects.join(&written[..2]).join(&written[2..]),
        objects.join("11").join(&id[2..]),
    )
    .unwrap();

    let err = run_target(dir.path(), &id).unwrap_err();
    assert!(matches!(&err, Error::RevisionNotFound(rev) if *rev == id));
}

#[test]
fn missing_head_commit_is_incomplete_history() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    let head = commit_at(dir.path(), "2026-04-10T10:00:00Z");
    delete_object(dir.path(), &head);

    for target in [None, Some("HEAD")] {
        let err = run(&Options {
            dir: dir.path(),
            target,
            branch: Some("main"),
            ..Options::default()
        })
        .unwrap_err();
        assert!(
            matches!(&err, Error::IncompleteHistory(msg) if msg.starts_with("HEAD ")),
            "{target:?}: {err}"
        );
    }
}

#[test]
fn symbolic_ref_to_missing_commit_is_incomplete_history() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    let tip = commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(
        dir.path(),
        &["symbolic-ref", "refs/heads/alias", "refs/heads/main"],
    );
    delete_object(dir.path(), &tip);

    let err = run_target(dir.path(), "alias").unwrap_err();
    assert!(
        matches!(&err, Error::IncompleteHistory(msg) if msg.starts_with("alias ")),
        "{err}"
    );
}

#[test]
fn explicit_head_in_empty_repository() {
    let dir = new_repo();
    let err = run_target(dir.path(), "HEAD").unwrap_err();
    assert!(matches!(err, Error::EmptyRepository));
}

#[test]
fn missing_target_commit_is_incomplete_history() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    let missing = commit_at(dir.path(), "2026-04-10T10:00:00Z");
    commit_at(dir.path(), "2026-04-10T11:00:00Z");
    tag_annotated(dir.path(), "v1", &missing);
    delete_object(dir.path(), &missing);

    for target in [missing.as_str(), "v1"] {
        let err = run_target(dir.path(), target).unwrap_err();
        assert!(
            matches!(&err, Error::IncompleteHistory(msg) if msg.starts_with(target)),
            "{target}: {err}"
        );
    }
}

#[test]
fn symbolic_branch_ref_is_followed() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(
        dir.path(),
        &["symbolic-ref", "refs/heads/trunk", "refs/heads/main"],
    );

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("trunk"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.2");
}

#[test]
fn symbolic_ref_cycle_is_unresolvable() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(
        dir.path(),
        &["symbolic-ref", "refs/heads/loop-a", "refs/heads/loop-b"],
    );
    git_in(
        dir.path(),
        &["symbolic-ref", "refs/heads/loop-b", "refs/heads/loop-a"],
    );

    let err = run(&Options {
        dir: dir.path(),
        branch: Some("loop-a"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NoDefaultBranch));
}

#[test]
fn head_on_symbolic_branch_alias_is_resolved() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(
        dir.path(),
        &["symbolic-ref", "refs/heads/alias", "refs/heads/main"],
    );
    git_in(dir.path(), &["symbolic-ref", "HEAD", "refs/heads/alias"]);

    for target in [None, Some("HEAD")] {
        let result = run(&Options {
            dir: dir.path(),
            target,
            branch: Some("main"),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(result, "20260410.2", "{target:?}");
    }
}

#[test]
fn head_on_dangling_symbolic_branch_alias_is_empty_repository() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(
        dir.path(),
        &["symbolic-ref", "refs/heads/alias", "refs/heads/unborn"],
    );
    git_in(dir.path(), &["symbolic-ref", "HEAD", "refs/heads/alias"]);

    for target in [None, Some("HEAD")] {
        let err = run(&Options {
            dir: dir.path(),
            target,
            branch: Some("main"),
            ..Options::default()
        })
        .unwrap_err();
        assert!(matches!(err, Error::EmptyRepository), "{target:?}: {err}");
    }
}

#[test]
fn detached_head_is_resolved() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "--detach"]);

    for target in [None, Some("HEAD")] {
        let result = run(&Options {
            dir: dir.path(),
            target,
            branch: Some("main"),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(result, "20260410.2", "{target:?}");
    }
}

#[test]
fn unknown_object_format_is_reported_as_unsupported() {
    let dir = new_repo();
    git_in(dir.path(), &["config", "core.repositoryformatversion", "1"]);
    git_in(dir.path(), &["config", "extensions.objectformat", "sha512"]);

    let err = run(&Options {
        dir: dir.path(),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(&err, Error::UnsupportedObjectFormat(name) if name == "sha512"));
    assert_eq!(
        err.to_string(),
        "unsupported object format sha512: only SHA-1 and SHA-256 repositories are supported"
    );
}

#[test]
fn sha256_repository_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    git_in(
        dir.path(),
        &["init", "-b", "main", "--object-format=sha256"],
    );
    let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    assert_eq!(hash.len(), 64);
    let opts = Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    };
    assert_eq!(run(&opts).unwrap(), "20260410.1");
    assert_eq!(
        run(&Options {
            target: Some("20260410.1"),
            ..opts
        })
        .unwrap(),
        hash
    );
    assert_eq!(
        run(&Options {
            target: Some("20260410.1"),
            short: true,
            ..opts
        })
        .unwrap(),
        hash[..7]
    );
    std::fs::write(dir.path().join("untracked"), "x").unwrap();
    assert_eq!(
        run(&Options {
            dirty_suffix: Some("-dirty"),
            ..opts
        })
        .unwrap(),
        format!("20260410.1-dirty.{}", &hash[..7])
    );
}

#[test]
fn repository_open_failure_surfaces_its_cause() {
    let dir = tempfile::tempdir().unwrap();
    let err = run(&Options {
        dir: &dir.path().join("does-not-exist"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::Git(_)));
}

#[test]
fn unusable_gitfile_below_a_repository_stops_discovery() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    for (name, contents) in [
        ("garbage", "garbage\n"),
        ("dangling", "gitdir: /nonexistent/gitdir\n"),
    ] {
        let sub = dir.path().join(name);
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join(".git"), contents).unwrap();
        let err = run(&Options {
            dir: &sub,
            branch: Some("main"),
            ..Options::default()
        })
        .unwrap_err();
        assert!(
            matches!(&err, Error::InvalidGitFile(path) if path.ends_with(format!("{name}/.git"))),
            "{name}: {err}"
        );
        assert!(err.to_string().ends_with(" is not a valid gitfile"));
    }
}

#[test]
fn unusable_dot_git_directory_is_stepped_over() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(sub.join(".git")).unwrap();
    assert_eq!(run_target(&sub, "HEAD").unwrap(), "20260410.1");
}

#[test]
fn gitfile_to_another_repository_is_followed() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let other = new_repo();
    commit_at(other.path(), "2026-04-11T12:00:00Z");
    commit_at(other.path(), "2026-04-11T13:00:00Z");
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(
        sub.join(".git"),
        format!("gitdir: {}\n", other.path().join(".git").display()),
    )
    .unwrap();
    assert_eq!(run_target(&sub, "HEAD").unwrap(), "20260411.2");
}

#[test]
fn repository_with_its_work_tree_elsewhere_is_found() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let work_tree = tempfile::tempdir().unwrap();
    git_in(
        dir.path(),
        &[
            "config",
            "core.worktree",
            work_tree.path().to_str().unwrap(),
        ],
    );
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    assert_eq!(run_target(&sub, "HEAD").unwrap(), "20260410.1");
}

fn ceiling_check(start: &std::path::Path, ceilings: Option<&std::ffi::OsStr>) -> Result<(), Error> {
    let repo = open_repo(start).unwrap();
    check_discovery(start, &repo, ceilings)
}

#[test]
fn ceiling_directory_is_never_searched() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let repo = dir.path().canonicalize().unwrap();
    let nested = repo.join("deep/er");
    std::fs::create_dir_all(&nested).unwrap();

    assert!(matches!(
        ceiling_check(&nested, Some(repo.as_os_str())),
        Err(Error::NotARepository)
    ));
    let list = std::env::join_paths(["/nonexistent", repo.to_str().unwrap()]).unwrap();
    assert!(matches!(
        ceiling_check(&nested, Some(&list)),
        Err(Error::NotARepository)
    ));

    ceiling_check(&nested, Some(repo.parent().unwrap().as_os_str())).unwrap();
    // An entry equal to the start directory is not an ancestor of it.
    ceiling_check(&nested, Some(nested.as_os_str())).unwrap();
    ceiling_check(&nested, Some("/nonexistent".as_ref())).unwrap();
    ceiling_check(&nested, None).unwrap();
}

#[cfg(unix)]
#[test]
fn relative_ceiling_entries_are_ignored() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let repo = dir.path().canonicalize().unwrap();
    let nested = repo.join("sub");
    std::fs::create_dir(&nested).unwrap();

    // Relative to this process's directory, this resolves to the repository.
    let depth = std::env::current_dir().unwrap().components().count() - 1;
    let relative = format!(
        "{}{}",
        "../".repeat(depth),
        repo.strip_prefix("/").unwrap().display()
    );
    ceiling_check(&nested, Some(relative.as_ref())).unwrap();
}

#[cfg(unix)]
#[test]
fn ceiling_directory_symlinks_are_resolved() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let repo = dir.path().canonicalize().unwrap();
    let nested = repo.join("sub");
    std::fs::create_dir(&nested).unwrap();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("link");
    std::os::unix::fs::symlink(&repo, &link).unwrap();

    assert!(matches!(
        ceiling_check(&nested, Some(link.as_os_str())),
        Err(Error::NotARepository)
    ));
}

#[test]
fn git_error_message_includes_its_sources() {
    #[derive(Debug, thiserror::Error)]
    #[error("outer")]
    struct Outer(#[source] std::io::Error);

    let err = git_err(Outer(std::io::Error::other("inner")));
    assert_eq!(err.to_string(), "outer: inner");
}

#[test]
fn newline_in_prefix_is_rejected() {
    // Rejected before the repository is even opened.
    let dir = tempfile::tempdir().unwrap();
    let err = run(&Options {
        dir: dir.path(),
        prefix: "a\nb",
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::InvalidPrefix));
}

#[test]
fn short_requires_reverse_mode() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    for target in [None, Some("HEAD")] {
        let err = run(&Options {
            dir: dir.path(),
            target,
            branch: Some("main"),
            short: true,
            ..Options::default()
        })
        .unwrap_err();
        assert!(matches!(err, Error::ShortRequiresReverse));
    }
}

#[test]
fn reverse_with_prefix() {
    let dir = new_repo();
    let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        target: Some("v0.20260410.1"),
        prefix: "v0.",
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, hash);
}

#[test]
fn reverse_requires_configured_prefix() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        target: Some("20260410.1"),
        prefix: "v0.",
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::MissingPrefix { .. }));
}

#[test]
fn invalid_version_date_is_not_a_revision() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["tag", "--no-sign", "20261301.1"]);
    let err = run_target(dir.path(), "20261301.1").unwrap_err();
    assert!(matches!(err, Error::InvalidVersionDate(_)));
}

#[test]
fn version_named_tag_does_not_shadow_reverse_lookup() {
    let dir = new_repo();
    let first = commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["tag", "--no-sign", "20260410.1"]);
    assert_eq!(run_target(dir.path(), "20260410.1").unwrap(), first);
}

#[test]
fn version_with_trailing_text_is_a_revision() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["tag", "--no-sign", "release-20260410.1"]);
    assert_eq!(
        run_target(dir.path(), "release-20260410.1").unwrap(),
        "20260410.2"
    );

    let err = run_target(dir.path(), "20260410.2-dirty.abc1234").unwrap_err();
    assert!(matches!(err, Error::RevisionNotFound(_)));
}

#[test]
fn not_traceable_unrelated_history() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "--orphan", "orphan"]);
    let orphan_hash = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        target: Some(&orphan_hash),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NotTraceable { .. }));
}

#[test]
fn head_on_different_branch() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T13:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        include_dirty_hash: false,
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1-dirty");
}

#[test]
fn reverse_decreasing_dates() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-11T12:00:00Z");
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let err = run(&Options {
        dir: dir.path(),
        target: Some("20260409.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::DecreasingDate { .. }));
}

// --- Anchor / chain membership ---

#[test]
fn merge_main_into_feature_then_ff_main_increases_version() {
    // The incident topology from the master plan: main accumulates several
    // same-date commits, then feature (branched before any of them) merges
    // main forward and main fast-forwards onto that merge. Under the old
    // first-parent-run count this reparents main's own commits off the
    // chain and the tip version can decrease; under the 0.3 cohort rule the
    // merge commit's count is the size of the whole reachable same-date set,
    // which only grows.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // c1
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    git_in(dir.path(), &["checkout", "main"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z"); // c2
    commit_at(dir.path(), "2026-04-10T11:00:00Z"); // c3
    commit_at(dir.path(), "2026-04-10T12:00:00Z"); // c4
    commit_at(dir.path(), "2026-04-10T13:00:00Z"); // c5
    git_in(dir.path(), &["checkout", "feature"]);
    merge_at(dir.path(), "main", "2026-04-10T14:00:00Z", "merge main");
    git_in(dir.path(), &["checkout", "main"]);
    git_in(dir.path(), &["merge", "feature"]); // fast-forward

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    // Cohort = {merge, c1, c5, c4, c3, c2}: every same-date commit reachable
    // through either parent, counted once. The pre-merge first-parent run at
    // c5 was 5 (c1..c5); this must not be smaller.
    assert_eq!(result, "20260410.6");
}

#[test]
fn reachable_off_chain_anchor_via_second_parent() {
    // Feature merges main forward without main having fast-forwarded yet.
    // The merge commit is off-chain, but it reaches main's tip directly
    // through its second parent, so the anchor must be main's tip itself,
    // not the pre-merge common ancestor.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // common ancestor
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z"); // feature's own commit
    git_in(dir.path(), &["checkout", "main"]);
    commit_at(dir.path(), "2026-04-10T11:00:00Z"); // main's second commit
    git_in(dir.path(), &["checkout", "feature"]);
    let merge = merge_at(dir.path(), "main", "2026-04-10T12:00:00Z", "merge main");

    let result = run(&Options {
        dir: dir.path(),
        target: Some(&merge),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    // Anchor is main's tip (cohort {common ancestor, main tip} = 2), not the
    // common ancestor alone (which would give count 1).
    assert!(result.starts_with("20260410.2-dirty."));
}

#[test]
fn merge_base_would_be_wrong_scenario() {
    // main merged in a side branch in the past; the target diverges from
    // that side branch's own commit, not from main's first-parent chain.
    // Raw merge-base(target, main-tip) returns the side-branch commit, which
    // is not a first-parent-chain member of main at all. The anchor
    // algorithm must instead find the true chain member the side branch
    // itself descends from.
    let dir = new_repo();
    let root = commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "side"]);
    let side_commit = commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    merge_at(dir.path(), "side", "2026-04-10T11:00:00Z", "merge side");
    commit_at(dir.path(), "2026-04-10T12:00:00Z");

    git_in(dir.path(), &["checkout", "-b", "off", &side_commit]);
    let target = commit_at(dir.path(), "2026-04-10T13:00:00Z");

    let result = run(&Options {
        dir: dir.path(),
        target: Some(&target),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    // The only true first-parent chain member reachable from `target` is the
    // root commit (cohort size 1); a merge-base-based anchor would have
    // wrongly used `side_commit`, which is not a chain member.
    assert!(result.starts_with("20260410.1-dirty."));
    let _ = root;
}

#[test]
fn merged_side_branch_commit_not_clean() {
    // A commit reachable from main but only through a merge's second parent
    // is off-chain, not a clean chain member, even though it is an ordinary
    // ancestor of the branch tip.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "side"]);
    let side_commit = commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    merge_at(dir.path(), "side", "2026-04-10T11:00:00Z", "merge side");

    let err = run(&Options {
        dir: dir.path(),
        target: Some(&side_commit),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::NotOnDefaultBranch { .. }));
}

#[test]
fn diverged_branch_anchor() {
    // Target and branch diverge from a common ancestor with no merge at all;
    // the anchor is that ancestor.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    let feature_hash = commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    commit_at(dir.path(), "2026-04-10T11:00:00Z");

    let result = run(&Options {
        dir: dir.path(),
        target: Some(&feature_hash),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    assert!(result.starts_with("20260410.1-dirty."));
}

#[test]
fn off_chain_target_with_diamond_ancestry_reaches_anchor() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // root, main's only commit
    git_in(dir.path(), &["checkout", "-b", "left"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z"); // shared ancestor
    git_in(dir.path(), &["checkout", "-b", "right"]);
    commit_at(dir.path(), "2026-04-10T11:00:00Z");
    git_in(dir.path(), &["checkout", "left"]);
    commit_at(dir.path(), "2026-04-10T11:30:00Z");
    let target = merge_at(dir.path(), "right", "2026-04-10T12:00:00Z", "merge right");
    // Equal arm times would make identical commits and `merge` a no-op.
    git_in(
        dir.path(),
        &["rev-parse", "--verify", &format!("{target}^2")],
    );

    let result = run(&Options {
        dir: dir.path(),
        target: Some(&target),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, format!("20260410.1-dirty.{}", &target[..7]));
}

// --- Bare repositories ---

fn bare_clone_of(src: &std::path::Path) -> tempfile::TempDir {
    let bare_parent = tempfile::tempdir().unwrap();
    let bare_path = bare_parent.path().join("repo.git");
    git_in(
        bare_parent.path(),
        &[
            "clone",
            "--bare",
            src.to_str().unwrap(),
            bare_path.to_str().unwrap(),
        ],
    );
    bare_parent
}

#[test]
fn bare_repository_implicit_head_no_workspace_check() {
    let src = new_repo();
    commit_at(src.path(), "2026-04-10T12:00:00Z");
    let bare_parent = bare_clone_of(src.path());
    let bare_path = bare_parent.path().join("repo.git");

    let result = run(&Options {
        dir: &bare_path,
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn bare_repository_explicit_revision() {
    let src = new_repo();
    let hash = commit_at(src.path(), "2026-04-10T12:00:00Z");
    let bare_parent = bare_clone_of(src.path());
    let bare_path = bare_parent.path().join("repo.git");

    let result = run(&Options {
        dir: &bare_path,
        target: Some(&hash),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn worktree_of_bare_repository_checks_workspace() {
    let src = new_repo();
    commit_at(src.path(), "2026-04-10T12:00:00Z");
    let bare_parent = bare_clone_of(src.path());
    let worktree = bare_parent.path().join("wt");
    git_in(
        &bare_parent.path().join("repo.git"),
        &["worktree", "add", worktree.to_str().unwrap(), "main"],
    );
    let run_in_worktree = |dirty_suffix| {
        run(&Options {
            dir: &worktree,
            branch: Some("main"),
            dirty_suffix,
            include_dirty_hash: false,
            ..Options::default()
        })
    };

    assert_eq!(run_in_worktree(None).unwrap(), "20260410.1");
    std::fs::write(worktree.join("untracked.txt"), "x").unwrap();
    assert!(matches!(
        run_in_worktree(None).unwrap_err(),
        Error::DirtyWorkspace
    ));
    assert_eq!(run_in_worktree(Some("-dirty")).unwrap(), "20260410.1-dirty");
}

#[test]
fn bare_repository_reverse_lookup() {
    let src = new_repo();
    let hash = commit_at(src.path(), "2026-04-10T12:00:00Z");
    let bare_parent = bare_clone_of(src.path());
    let bare_path = bare_parent.path().join("repo.git");

    let result = run(&Options {
        dir: &bare_path,
        target: Some("20260410.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, hash);
}

#[test]
fn reverse_requires_exact_version_and_prefix() {
    let dir = new_repo();
    let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let opts = Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    };
    for target in [
        "",
        "garbage\n20260410.1",
        "20260410.1rc1",
        "20260410.1-dirty.abcdef0",
        "v0.20260410.1",
    ] {
        assert!(
            matches!(
                run(&Options {
                    target: Some(target),
                    ..opts
                }),
                Err(Error::RevisionNotFound(_))
            ),
            "{target:?}"
        );
    }
    // A prefix may itself look like a version or contain slashes.
    for prefix in ["v0.", "20200101.5/", "release/"] {
        let target = format!("{prefix}20260410.1");
        assert_eq!(
            run(&Options {
                target: Some(&target),
                prefix,
                ..opts
            })
            .unwrap(),
            hash
        );
    }
}

#[test]
fn short_rejects_version_like_revision() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["tag", "--no-sign", "20260410.1rc1"]);
    let opts = Options {
        dir: dir.path(),
        target: Some("20260410.1rc1"),
        branch: Some("main"),
        ..Options::default()
    };
    assert_eq!(run(&opts).unwrap(), "20260410.1");
    let err = run(&Options {
        short: true,
        ..opts
    })
    .unwrap_err();
    assert!(matches!(err, Error::ShortRequiresReverse));
}

#[test]
fn packed_repository_round_trip() {
    let dir = new_repo();
    std::fs::write(dir.path().join("file"), "content").unwrap();
    git_in(dir.path(), &["add", "file"]);
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let hash = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    git_in(dir.path(), &["repack", "-ad"]);
    git_in(dir.path(), &["prune-packed"]);
    git_in(dir.path(), &["pack-refs", "--all"]);
    assert!(
        !dir.path()
            .join(".git/objects")
            .join(&hash[..2])
            .join(&hash[2..])
            .exists()
    );

    let opts = Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    };
    assert_eq!(run(&opts).unwrap(), "20260410.2");
    assert_eq!(
        run(&Options {
            target: Some("20260410.2"),
            ..opts
        })
        .unwrap(),
        hash
    );
}

#[test]
fn abbreviated_revisions_in_both_object_formats() {
    for algorithm in ["sha1", "sha256"] {
        let dir = tempfile::tempdir().unwrap();
        git_in(
            dir.path(),
            &[
                "init",
                "-b",
                "main",
                &format!("--object-format={algorithm}"),
            ],
        );
        let first = commit_at(dir.path(), "2026-04-10T12:00:00Z");
        let second = commit_at(dir.path(), "2026-04-10T13:00:00Z");
        let opts = Options {
            dir: dir.path(),
            branch: Some("main"),
            ..Options::default()
        };
        for packed in [false, true] {
            if packed {
                git_in(dir.path(), &["repack", "-ad"]);
                git_in(dir.path(), &["prune-packed"]);
            }
            for target in [
                first[..7].to_owned(),
                first[..7].to_ascii_uppercase(),
                format!("{}~1", &second[..7]),
                format!("{}^", &second[..7]),
            ] {
                assert_eq!(
                    run(&Options {
                        target: Some(&target),
                        ..opts
                    })
                    .unwrap(),
                    "20260410.1",
                    "{algorithm} {target}"
                );
            }
        }
        git_in(dir.path(), &["tag", "--no-sign", &first[..7], &second]);
        assert_eq!(
            run(&Options {
                target: Some(&first[..7]),
                ..opts
            })
            .unwrap(),
            "20260410.2"
        );
        assert!(matches!(
            run(&Options {
                target: Some("0000000"),
                ..opts
            }),
            Err(Error::RevisionNotFound(_))
        ));
    }
}

#[test]
fn hex_prefixed_ref_names_resolve_in_both_object_formats() {
    for algorithm in ["sha1", "sha256"] {
        let dir = tempfile::tempdir().unwrap();
        git_in(
            dir.path(),
            &[
                "init",
                "-b",
                "main",
                &format!("--object-format={algorithm}"),
            ],
        );
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        git_in(dir.path(), &["branch", "cafe@work"]);
        assert_eq!(
            run_target(dir.path(), "cafe@work").unwrap(),
            "20260410.1",
            "{algorithm}"
        );
    }
}

#[test]
fn object_id_of_the_other_format_is_not_a_revision() {
    for (algorithm, other_len) in [("sha1", 64), ("sha256", 40)] {
        let dir = tempfile::tempdir().unwrap();
        git_in(
            dir.path(),
            &[
                "init",
                "-b",
                "main",
                &format!("--object-format={algorithm}"),
            ],
        );
        let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
        let foreign = "ab".repeat(other_len / 2);
        let err = run_target(dir.path(), &foreign).unwrap_err();
        assert!(matches!(err, Error::RevisionNotFound(_)), "{algorithm}");

        // A ref named like such an ID is still an ordinary ref.
        git_in(
            dir.path(),
            &["update-ref", &format!("refs/tags/{foreign}"), &hash],
        );
        assert_eq!(
            run_target(dir.path(), &foreign).unwrap(),
            "20260410.1",
            "{algorithm}"
        );
    }
}

#[test]
fn linked_worktree_round_trip_and_dirty() {
    let dir = new_repo();
    std::fs::write(dir.path().join("file"), "content").unwrap();
    git_in(dir.path(), &["add", "file"]);
    let hash = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let parent = tempfile::tempdir().unwrap();
    let linked = parent.path().join("linked");
    git_in(
        dir.path(),
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    let opts = Options {
        dir: &linked,
        branch: Some("main"),
        ..Options::default()
    };
    assert_eq!(run(&opts).unwrap(), "20260410.1");
    assert_eq!(
        run(&Options {
            target: Some("20260410.1"),
            ..opts
        })
        .unwrap(),
        hash
    );
    std::fs::write(linked.join("file"), "changed").unwrap();
    assert!(matches!(run(&opts), Err(Error::DirtyWorkspace)));
    assert_eq!(
        run(&Options {
            dirty_suffix: Some("-dirty"),
            ..opts
        })
        .unwrap(),
        format!("20260410.1-dirty.{}", &hash[..7])
    );
}

// --- Incomplete history: graft, replace, shallow, partial clone ---

#[test]
fn graft_file_rejected() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    std::fs::write(dir.path().join(".git/info/grafts"), "").unwrap();

    let err = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn replace_ref_ignored() {
    // If replace refs were honored, `c2` would appear to have `c1`'s
    // content (no parent, 2026-04-09), producing "20260409.1" instead.
    // Rust tests run multithreaded in one process; using a config override
    // rather than an environment variable to disable replace-refs means
    // this needs no process-global state and cannot leak across tests.
    // The repository's own core.useReplaceRefs must not matter either way:
    // the API-level override of that same key outranks repo config, so all
    // three permutations must agree.
    for repo_config in [None, Some("true"), Some("false")] {
        let dir = new_repo();
        let c1 = commit_at(dir.path(), "2026-04-09T12:00:00Z");
        let c2 = commit_at(dir.path(), "2026-04-10T12:00:00Z");
        git_in(dir.path(), &["replace", &c2, &c1]);
        if let Some(value) = repo_config {
            git_in(dir.path(), &["config", "core.useReplaceRefs", value]);
        }

        let result = run(&Options {
            dir: dir.path(),
            branch: Some("main"),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(result, "20260410.1", "repo config: {repo_config:?}");
    }
}

#[test]
fn anchor_found_but_unexplored_path_is_incomplete_history() {
    // The off-chain target reaches a chain anchor through one parent, but
    // its other parent's object is missing: the unexplored path could have
    // reached a newer chain member, so the found anchor is unproven and
    // the result must be incomplete history, not a dirty version.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "side"]);
    let missing = commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature", "main"]);
    commit_at(dir.path(), "2026-04-10T11:00:00Z");
    let target = merge_at(dir.path(), "side", "2026-04-10T12:00:00Z", "merge side");
    git_in(dir.path(), &["checkout", "main"]);
    git_in(dir.path(), &["branch", "-D", "side"]);
    delete_object(dir.path(), &missing);

    let err = run(&Options {
        dir: dir.path(),
        target: Some(&target),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn missing_ancestor_off_chain_is_incomplete_history() {
    // The target's own ancestry (not the branch's) has a missing commit.
    // Anchor resolution cannot conclude not-traceable, since it never
    // proved the rest of the target's history is unrelated.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "--orphan", "orphan"]);
    let missing = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    let target = commit_at(dir.path(), "2026-04-10T14:00:00Z");
    delete_object(dir.path(), &missing);

    let err = run(&Options {
        dir: dir.path(),
        target: Some(&target),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn branch_tip_missing_makes_relationship_unprovable() {
    // The branch tip's own commit object is missing. Even though the
    // target's history is completely resolvable and shares nothing with
    // the (unreadable) branch, we cannot conclusively rule out a
    // relationship without reading the branch's own history first.
    let dir = new_repo();
    let main_tip = commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(dir.path(), &["checkout", "--orphan", "orphan"]);
    let target = commit_at(dir.path(), "2026-04-10T13:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    delete_object(dir.path(), &main_tip);

    let err = run(&Options {
        dir: dir.path(),
        target: Some(&target),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn shallow_forward_succeeds_with_older_date_boundary() {
    let src = new_repo();
    commit_at(src.path(), "2026-04-08T12:00:00Z");
    commit_at(src.path(), "2026-04-09T12:00:00Z");
    commit_at(src.path(), "2026-04-10T12:00:00Z");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &[
            "clone",
            "--depth=2",
            "--no-single-branch",
            &file_url(src.path()),
            "shallow",
        ],
    );
    let shallow = parent.path().join("shallow");

    let result = run(&Options {
        dir: &shallow,
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    // The shallow boundary (the first commit, 04-08) is beyond the fetched,
    // older-dated 04-09 commit that already proves the block's boundary.
    assert_eq!(result, "20260410.1");
}

#[test]
fn shallow_forward_rejects_boundary_inside_date_block() {
    let src = new_repo();
    commit_at(src.path(), "2026-04-08T12:00:00Z");
    commit_at(src.path(), "2026-04-10T12:00:00Z");
    commit_at(src.path(), "2026-04-10T13:00:00Z");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &[
            "clone",
            "--depth=2",
            "--no-single-branch",
            &file_url(src.path()),
            "shallow",
        ],
    );
    let shallow = parent.path().join("shallow");

    let err = run(&Options {
        dir: &shallow,
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn shallow_reverse_rejects_incomplete_older_date_block() {
    let src = new_repo();
    commit_at(src.path(), "2026-04-08T12:00:00Z");
    commit_at(src.path(), "2026-04-10T12:00:00Z");
    commit_at(src.path(), "2026-04-10T13:00:00Z");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &[
            "clone",
            "--depth=2",
            "--no-single-branch",
            &file_url(src.path()),
            "shallow",
        ],
    );
    let shallow = parent.path().join("shallow");

    let err = run(&Options {
        dir: &shallow,
        target: Some("20260410.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn shallow_second_parent_same_date_exits_incomplete() {
    // At depth 3, both the older root and the same-date side-base become
    // shallow boundaries. The root is pruned regardless, but side-base is a
    // same-date cohort member reached only through the merge's second
    // parent, so its true root/shallow-cut status must be provable -- this
    // exercises the multi-parent walk against a real .git/shallow file.
    let src = new_repo();
    commit_at(src.path(), "2026-04-08T09:00:00Z"); // root, older
    git_in(src.path(), &["checkout", "-b", "side"]);
    commit_at(src.path(), "2026-04-10T09:00:00Z"); // side-base, same date
    commit_at(src.path(), "2026-04-10T09:30:00Z"); // side-1
    git_in(src.path(), &["checkout", "main"]);
    commit_at(src.path(), "2026-04-10T09:15:00Z"); // main-1
    merge_at(src.path(), "side", "2026-04-10T10:00:00Z", "merge");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &[
            "clone",
            "--depth=3",
            "--single-branch",
            "--branch=main",
            &file_url(src.path()),
            "shallow",
        ],
    );
    let shallow = parent.path().join("shallow");

    let err = run(&Options {
        dir: &shallow,
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn shallow_second_parent_older_date_succeeds() {
    // Same shape, but side-base is dated the day before: every shallow
    // boundary is strictly older than the target date, so nothing needs
    // disambiguation and the cohort is provable.
    let src = new_repo();
    commit_at(src.path(), "2026-04-08T09:00:00Z"); // root, older
    git_in(src.path(), &["checkout", "-b", "side"]);
    commit_at(src.path(), "2026-04-09T09:00:00Z"); // side-base, older date
    commit_at(src.path(), "2026-04-10T09:30:00Z"); // side-1
    git_in(src.path(), &["checkout", "main"]);
    commit_at(src.path(), "2026-04-10T09:15:00Z"); // main-1
    merge_at(src.path(), "side", "2026-04-10T10:00:00Z", "merge");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &[
            "clone",
            "--depth=3",
            "--single-branch",
            "--branch=main",
            &file_url(src.path()),
            "shallow",
        ],
    );
    let shallow = parent.path().join("shallow");

    let result = run(&Options {
        dir: &shallow,
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    // Cohort = {merge, main-1, side-1}.
    assert_eq!(result, "20260410.3");
}

#[test]
fn partial_clone_blob_filter_succeeds_offline() {
    let src = new_repo();
    std::fs::write(src.path().join("file.txt"), "content").unwrap();
    git_in(src.path(), &["add", "file.txt"]);
    let hash = commit_at(src.path(), "2026-04-10T12:00:00Z");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &[
            "clone",
            "--filter=blob:none",
            &file_url(src.path()),
            "partial",
        ],
    );
    let partial = parent.path().join("partial");

    let result = run(&Options {
        dir: &partial,
        target: Some(&hash),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

// --- 0.3 forward cohort counting ---

#[test]
fn cohort_counts_any_parent_same_date() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    merge_at(dir.path(), "feature", "2026-04-10T11:00:00Z", "merge");

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    // {root, feature commit, merge} all share the date.
    assert_eq!(result, "20260410.3");
}

#[test]
fn cohort_same_date_behind_older_not_counted() {
    // A same-date commit that is only reachable by first passing through an
    // older-dated one is never examined: pruning stops the walk before it
    // would be seen. `side`'s tip is older-dated than the eventual merge, but
    // its own parent (committed earlier, despite the child's later date) has
    // the merge's date; that parent must never be counted, because the walk
    // never gets past the older tip to see it.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-09T09:00:00Z"); // root
    git_in(dir.path(), &["checkout", "-b", "side"]);
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // hidden: same date as the merge
    commit_at(dir.path(), "2026-04-08T09:00:00Z"); // side tip: older than the merge
    git_in(dir.path(), &["checkout", "main"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    merge_at(dir.path(), "side", "2026-04-10T11:00:00Z", "merge side");

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    // Cohort = {merge, main's own 04-10 commit}: `side`'s older tip prunes
    // before its hidden same-date parent is ever reached.
    assert_eq!(result, "20260410.2");
}

#[test]
fn cohort_near_boundary_newer_date_rejected() {
    // A future-dated commit arrives through a merge's second parent after
    // a same-date first-parent member has already been counted: the
    // strictly-newer rejection must apply to every parent edge the walk
    // visits, not just first parents, and reverse lookup must surface the
    // same error through the block member's own cohort computation (the
    // first-parent chain's dates are perfectly monotonic here).
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // root
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-11T09:00:00Z"); // dated tomorrow
    git_in(dir.path(), &["checkout", "main"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    merge_at(dir.path(), "feature", "2026-04-10T11:00:00Z", "merge");

    let err = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::DecreasingDate { .. }));

    let err = run(&Options {
        dir: dir.path(),
        target: Some("20260410.3"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::DecreasingDate { .. }));
}

#[test]
fn cohort_diamond_merge_counts_commit_once() {
    // A commit reachable via two different merge paths is counted once.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "left"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    git_in(dir.path(), &["checkout", "-b", "right", "main"]);
    merge_at(
        dir.path(),
        "left",
        "2026-04-10T11:00:00Z",
        "right merges left",
    );
    git_in(dir.path(), &["checkout", "main"]);
    merge_at(
        dir.path(),
        "left",
        "2026-04-10T12:00:00Z",
        "main merges left",
    );
    merge_at(
        dir.path(),
        "right",
        "2026-04-10T13:00:00Z",
        "main merges right",
    );

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    // {root, left, right-merge, main-merges-left, main-merges-right} = 5,
    // even though `root` and `left` are each reachable via multiple paths.
    assert_eq!(result, "20260410.5");
}

#[test]
fn cohort_off_chain_anchor_uses_anchors_own_cohort_size() {
    // The anchor is a merge whose cohort {merge, side, root} = 3 differs
    // from its first-parent chain position (2), so a regression to
    // position-based counting for the dirty base version would surface
    // here as "20260410.2-dirty".
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // root
    git_in(dir.path(), &["checkout", "-b", "side"]);
    commit_at(dir.path(), "2026-04-10T09:30:00Z"); // side
    git_in(dir.path(), &["checkout", "main"]);
    merge_at(dir.path(), "side", "2026-04-10T10:00:00Z", "merge side");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T11:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);

    let result = run(&Options {
        dir: dir.path(),
        target: Some("feature"),
        branch: Some("main"),
        dirty_suffix: Some("-dirty"),
        include_dirty_hash: false,
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.3-dirty");
}

#[test]
fn cohort_cross_day_merge_second_parent_not_counted() {
    // The simplest shape: a merge whose second parent is dated on an
    // entirely older day contributes nothing to the merge's cohort.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // root
    git_in(dir.path(), &["checkout", "-b", "side"]);
    commit_at(dir.path(), "2026-04-09T12:00:00Z"); // side: the day before
    git_in(dir.path(), &["checkout", "main"]);
    merge_at(dir.path(), "side", "2026-04-10T10:00:00Z", "merge side");

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    // Cohort = {merge, root}; the older-dated side commit is pruned.
    assert_eq!(result, "20260410.2");
}

#[test]
fn cohort_newer_date_behind_older_date_tolerated() {
    // A future-dated commit buried behind a strictly-older one is never
    // visited: the walk prunes at the older commit first, so the anomaly
    // cannot be seen, and must not be rejected.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-15T09:00:00Z"); // buried root, "future"
    commit_at(dir.path(), "2026-04-10T09:00:00Z"); // older child (clock fixed)
    commit_at(dir.path(), "2026-04-10T10:00:00Z"); // same date as its parent
    commit_at(dir.path(), "2026-04-11T09:00:00Z"); // tip

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260411.1");
}

#[test]
fn d2_d2_d1_d2_forward_succeeds_reverse_dies_decreasing() {
    // Chain dates, oldest to newest: D2, D1, D2, D2 -- the root is dated
    // after its own child. Forward at the tip succeeds (the D1 commit
    // prunes the walk before the anomalous root is seen); reverse lookup
    // for the D1 date walks the first-parent chain past D1 into the root
    // and must still report the decreasing sequence.
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T08:00:00Z"); // root: D2, anomalous
    commit_at(dir.path(), "2026-04-09T09:00:00Z"); // D1
    commit_at(dir.path(), "2026-04-10T09:30:00Z"); // D2
    commit_at(dir.path(), "2026-04-10T10:00:00Z"); // tip: D2

    let result = run(&Options {
        dir: dir.path(),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.2");

    let err = run(&Options {
        dir: dir.path(),
        target: Some("20260409.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::DecreasingDate { .. }));
}

// --- 0.3 reverse sparse lookup ---

#[test]
fn reverse_sparse_sequence_finds_exact_cohort_match() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    let merge = merge_at(dir.path(), "feature", "2026-04-10T11:00:00Z", "merge");

    let result = run(&Options {
        dir: dir.path(),
        target: Some("20260410.3"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, merge);
}

#[test]
fn reverse_sparse_sequence_no_exact_match_returns_not_found() {
    let dir = new_repo();
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    git_in(dir.path(), &["checkout", "-b", "feature"]);
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    git_in(dir.path(), &["checkout", "main"]);
    merge_at(dir.path(), "feature", "2026-04-10T11:00:00Z", "merge");

    // The sequence here is sparse (only .1 and .3 exist); .2 must not
    // resolve to the nearest match.
    let err = run(&Options {
        dir: dir.path(),
        target: Some("20260410.2"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::VersionNotFound(_)));
}

#[test]
fn reverse_missing_block_boundary_object_exits_incomplete() {
    // The date block's older boundary commit object is missing outright
    // (no shallow mark involved): the block walk must report incomplete
    // history when it cannot load the next chain commit.
    let dir = new_repo();
    let boundary = commit_at(dir.path(), "2026-04-08T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T09:00:00Z");
    commit_at(dir.path(), "2026-04-10T10:00:00Z");
    delete_object(dir.path(), &boundary);

    let err = run(&Options {
        dir: dir.path(),
        target: Some("20260410.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

#[test]
fn reverse_sparse_sequence_early_stop_skips_later_members() {
    // Reuses the shallow second-parent topology: the block members on
    // main's chain are [main-1, merge], and only the merge's cohort needs
    // the unprovable shallow cut. Resolving 20260410.1 at main-1 proves
    // the oldest-first scan returns before ever evaluating the merge;
    // 20260410.2 forces the merge's cohort and must surface exit 4, not
    // "version not found".
    let src = new_repo();
    commit_at(src.path(), "2026-04-08T09:00:00Z"); // root, older
    git_in(src.path(), &["checkout", "-b", "side"]);
    commit_at(src.path(), "2026-04-10T09:00:00Z"); // side-base, same date
    commit_at(src.path(), "2026-04-10T09:30:00Z"); // side-1
    git_in(src.path(), &["checkout", "main"]);
    commit_at(src.path(), "2026-04-10T09:15:00Z"); // main-1
    merge_at(src.path(), "side", "2026-04-10T10:00:00Z", "merge");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &[
            "clone",
            "--depth=3",
            "--single-branch",
            "--branch=main",
            &file_url(src.path()),
            "shallow",
        ],
    );
    let shallow = parent.path().join("shallow");
    let main_1 = git_in(&shallow, &["rev-parse", "HEAD~1"]);

    let result = run(&Options {
        dir: &shallow,
        target: Some("20260410.1"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, main_1);

    let err = run(&Options {
        dir: &shallow,
        target: Some("20260410.2"),
        branch: Some("main"),
        ..Options::default()
    })
    .unwrap_err();
    assert!(matches!(err, Error::IncompleteHistory(_)));
}

// --- `--remote` and branch detection ---

#[test]
fn remote_flag_selects_named_remote() {
    let remote_src = new_repo();
    commit_at(remote_src.path(), "2026-04-10T12:00:00Z");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &["clone", remote_src.path().to_str().unwrap(), "local"],
    );
    let local = parent.path().join("local");
    git_in(&local, &["remote", "rename", "origin", "upstream"]);
    commit_at(&local, "2026-04-10T13:00:00Z");

    let result = run(&Options {
        dir: &local,
        remote: Some("upstream"),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.2");
}

#[test]
fn detect_local_master_fallback() {
    let dir = tempfile::tempdir().unwrap();
    git_in(dir.path(), &["init", "-b", "master"]);
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    let result = run(&Options {
        dir: dir.path(),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn dangling_symbolic_branches_are_not_default_branch_candidates() {
    let dir = tempfile::tempdir().unwrap();
    git_in(dir.path(), &["init", "-b", "master"]);
    commit_at(dir.path(), "2026-04-10T12:00:00Z");
    git_in(
        dir.path(),
        &["update-ref", "refs/remotes/origin/master", "HEAD"],
    );
    for name in ["refs/heads/main", "refs/remotes/origin/main"] {
        git_in(dir.path(), &["symbolic-ref", name, "refs/nowhere"]);
    }

    let result = run(&Options {
        dir: dir.path(),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn detect_remote_main_without_symbolic_head() {
    // A plain `remote add` + `fetch` (unlike `clone`) still creates the
    // cached remote-tracking branch, but nothing here relies on a symbolic
    // `origin/HEAD`; that tier of detection is exercised only when the ref
    // is genuinely absent.
    let remote = new_repo();
    commit_at(remote.path(), "2026-04-10T12:00:00Z");
    let local = new_repo();
    commit_at(local.path(), "2026-04-10T13:00:00Z");
    git_in(
        local.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git_in(local.path(), &["fetch", "origin"]);
    git_in(local.path(), &["remote", "set-head", "origin", "-d"]);

    let result = run(&Options {
        dir: local.path(),
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.1");
}

#[test]
fn origin_head_slash_branch_name() {
    let remote = tempfile::tempdir().unwrap();
    git_in(remote.path(), &["init", "-b", "release/v1"]);
    commit_at(remote.path(), "2026-04-10T12:00:00Z");

    let parent = tempfile::tempdir().unwrap();
    git_in(
        parent.path(),
        &["clone", remote.path().to_str().unwrap(), "local"],
    );
    let local = parent.path().join("local");
    commit_at(&local, "2026-04-10T13:00:00Z");

    let result = run(&Options {
        dir: &local,
        ..Options::default()
    })
    .unwrap();
    assert_eq!(result, "20260410.2");
}

// patch_manifest tests

const SRC_WITH_COMMENT: &str = "\
[package]
# Load-bearing comment that must survive round-trip.
name = \"demo\"
edition = \"2024\"
";

#[test]
fn patch_inserts_version_and_publish() {
    let out = patch_manifest(SRC_WITH_COMMENT, "0.20260518.1").unwrap();
    assert!(out.contains("# Load-bearing comment"));
    assert!(out.contains("version = \"0.20260518.1\""));
    assert!(out.contains("publish = true"));
}

#[test]
fn patch_preserves_trailing_newline() {
    let out = patch_manifest(SRC_WITH_COMMENT, "0.20260518.1").unwrap();
    assert!(out.ends_with('\n'));
}

#[test]
fn patch_idempotent_on_matching_version() {
    let once = patch_manifest(SRC_WITH_COMMENT, "0.20260518.1").unwrap();
    let twice = patch_manifest(&once, "0.20260518.1").unwrap();
    assert_eq!(once, twice);
}

#[test]
fn patch_rejects_mismatched_version() {
    let src = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
    let err = patch_manifest(src, "0.2.0").unwrap_err();
    assert!(matches!(
        err,
        PatchError::VersionMismatch { ref existing, ref computed }
            if existing == "0.1.0" && computed == "0.2.0"
    ));
}

#[test]
fn patch_rejects_publish_false() {
    let src = "[package]\nname = \"demo\"\npublish = false\n";
    let err = patch_manifest(src, "0.1.0").unwrap_err();
    assert!(matches!(err, PatchError::PublishFalse));
}

#[test]
fn patch_accepts_publish_true() {
    let src = "[package]\nname = \"demo\"\npublish = true\n";
    let out = patch_manifest(src, "0.1.0").unwrap();
    assert!(out.contains("version = \"0.1.0\""));
    assert!(out.matches("publish = true").count() == 1);
}

#[test]
fn patch_accepts_publish_registry_list() {
    let src = "[package]\nname = \"demo\"\npublish = [\"my-registry\"]\n";
    let out = patch_manifest(src, "0.1.0").unwrap();
    assert!(out.contains("version = \"0.1.0\""));
    assert!(out.contains("publish = [\"my-registry\"]"));
}

#[test]
fn patch_rejects_missing_package() {
    let src = "[dependencies]\nfoo = \"1\"\n";
    let err = patch_manifest(src, "0.1.0").unwrap_err();
    assert!(matches!(err, PatchError::MissingPackage));
}

#[test]
fn patch_rejects_workspace_version_inheritance() {
    let src = "[package]\nname = \"demo\"\nversion.workspace = true\n";
    let err = patch_manifest(src, "0.1.0").unwrap_err();
    assert!(matches!(err, PatchError::WorkspaceInheritance));
}

#[test]
fn patch_rejects_workspace_publish_inheritance() {
    let src = "[package]\nname = \"demo\"\npublish.workspace = true\n";
    let err = patch_manifest(src, "0.1.0").unwrap_err();
    assert!(matches!(err, PatchError::WorkspaceInheritance));
}

#[test]
fn patch_rejects_malformed_toml() {
    let src = "[package\nname = \"demo\"\n";
    let err = patch_manifest(src, "0.1.0").unwrap_err();
    assert!(matches!(err, PatchError::Parse(_)));
}

#[test]
fn patch_error_display() {
    assert_eq!(
        PatchError::MissingPackage.to_string(),
        "manifest has no `[package]` section"
    );
    assert_eq!(
        PatchError::WorkspaceInheritance.to_string(),
        "workspace version inheritance not supported; set version in package manifest"
    );
    assert_eq!(
        PatchError::PublishFalse.to_string(),
        "manifest sets `publish = false`; refusing to overwrite"
    );
    let mismatch = PatchError::VersionMismatch {
        existing: "0.1.0".into(),
        computed: "0.2.0".into(),
    };
    assert!(mismatch.to_string().contains("0.1.0"));
    assert!(mismatch.to_string().contains("0.2.0"));
    let parse_err = patch_manifest("[package\n", "0.1.0").unwrap_err();
    assert!(parse_err.to_string().starts_with("invalid TOML:"));
}
