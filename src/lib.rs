use gix::ObjectId;
use gix::prelude::ObjectIdExt;
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("workspace is dirty (use --dirty to produce a dirty version)")]
    DirtyWorkspace,

    #[error("{subject} is not on the default branch ({branch})")]
    NotOnDefaultBranch { subject: String, branch: String },

    #[error("{subject} has no common history with the default branch ({branch})")]
    NotTraceable {
        subject: String,
        branch: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("cannot determine default branch")]
    NoDefaultBranch,

    #[error("committer dates are not non-decreasing (found {earlier} after {head} in history)")]
    DecreasingDate { head: String, earlier: String },

    #[error("no commits in repository")]
    EmptyRepository,

    #[error("not a git repository")]
    NotARepository,

    #[error("not a gitcalver version or git revision: {0}")]
    RevisionNotFound(String),

    #[error("version not found: {0}")]
    VersionNotFound(String),

    #[error("{0}")]
    Git(Box<dyn std::error::Error + Send + Sync>),
}

pub struct Options<'a> {
    pub dir: &'a std::path::Path,
    pub target: Option<&'a str>,
    pub prefix: &'a str,
    pub dirty_suffix: Option<&'a str>,
    pub include_dirty_hash: bool,
    pub branch: Option<&'a str>,
    pub short: bool,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            dir: std::path::Path::new("."),
            target: None,
            prefix: "",
            dirty_suffix: None,
            include_dirty_hash: true,
            branch: None,
            short: false,
        }
    }
}

/// Compute a gitcalver version string or resolve a version to a commit.
///
/// If `target` parses as a gitcalver version (e.g., `20260412.1`), reverse
/// mode is used regardless of whether the string also matches a git ref.
///
/// Version dates are always UTC, regardless of the committer's timezone offset.
///
/// # Errors
///
/// Returns `Error` if the directory is not a git repository, the workspace is
/// dirty (unless allowed), the target is not on the default branch, or commit
/// history is malformed.
pub fn run(opts: &Options<'_>) -> Result<String, Error> {
    let repo = gix::discover(opts.dir).map_err(|_| Error::NotARepository)?;

    if let Some(target) = opts.target
        && let Some((date_str, n)) = parse_version(target)
    {
        return reverse(&repo, opts, date_str, n);
    }

    forward(&repo, opts)
}

fn forward(repo: &gix::Repository, opts: &Options<'_>) -> Result<String, Error> {
    let is_head = opts.target.is_none();

    let target_id = match opts.target {
        None => repo.head_id().map_err(|_| Error::EmptyRepository)?.detach(),
        Some(rev) => repo
            .rev_parse_single(rev)
            .map_err(|_| Error::RevisionNotFound(rev.to_owned()))?
            .detach(),
    };

    let subject = opts.target.unwrap_or("HEAD");
    let branch = detect_branch(repo, opts.branch)?;
    let relation = branch_relation(repo, target_id, &branch, is_head).map_err(|source| {
        Error::NotTraceable {
            subject: subject.to_owned(),
            branch: branch.name.clone(),
            source,
        }
    })?;

    let (version_commit, off_branch) = match relation {
        BranchRelation::OnBranch => (target_id, false),
        BranchRelation::OffBranch { merge_base } => (merge_base, true),
    };

    let workspace_dirty = if is_head { check_dirty(repo)? } else { false };
    let dirty = workspace_dirty || off_branch;

    if dirty && opts.dirty_suffix.is_none() {
        return if off_branch {
            Err(Error::NotOnDefaultBranch {
                subject: subject.to_owned(),
                branch: branch.name,
            })
        } else {
            Err(Error::DirtyWorkspace)
        };
    }

    let (date, count) = walk_first_parent(repo, version_commit)?;

    let hash = if dirty && opts.include_dirty_hash {
        short_hash(repo, target_id)
    } else {
        String::new()
    };

    Ok(format_version(
        opts.prefix,
        &date,
        count,
        dirty,
        opts.dirty_suffix.unwrap_or(""),
        &hash,
    ))
}

fn reverse(
    repo: &gix::Repository,
    opts: &Options<'_>,
    date_str: &str,
    n: usize,
) -> Result<String, Error> {
    let branch = detect_branch(repo, opts.branch)?;

    let mut candidates = Vec::new();
    let mut current = branch.hash;
    let mut prev_date: Option<String> = None;

    loop {
        let commit = repo.find_commit(current).map_err(git_err)?;
        let commit_date = committer_date(&commit)?;

        if let Some(ref prev) = prev_date
            && commit_date.as_str() > prev.as_str()
        {
            return Err(Error::DecreasingDate {
                head: prev.clone(),
                earlier: commit_date,
            });
        }

        if commit_date == date_str {
            candidates.push(current);
        } else if commit_date.as_str() < date_str {
            break;
        }

        prev_date = Some(commit_date);

        match first_parent_id(&commit) {
            Some(parent) => current = parent,
            None => break,
        }
    }

    if n > candidates.len() {
        return Err(Error::VersionNotFound(
            opts.target.unwrap_or_default().to_owned(),
        ));
    }

    // N=1 is oldest on that date, N=len is newest; candidates are newest-first.
    let target_hash = candidates[candidates.len() - n];

    if opts.short {
        Ok(short_hash(repo, target_hash))
    } else {
        Ok(target_hash.to_string())
    }
}

fn walk_first_parent(repo: &gix::Repository, start: ObjectId) -> Result<(String, usize), Error> {
    let commit = repo.find_commit(start).map_err(git_err)?;
    let date = committer_date(&commit)?;
    let mut count = 1;
    let mut current_commit = commit;

    while let Some(parent_id) = first_parent_id(&current_commit) {
        let parent = repo.find_commit(parent_id).map_err(git_err)?;
        let parent_date = committer_date(&parent)?;

        match parent_date.cmp(&date) {
            std::cmp::Ordering::Equal => {
                count += 1;
                current_commit = parent;
            }
            std::cmp::Ordering::Greater => {
                return Err(Error::DecreasingDate {
                    head: date,
                    earlier: parent_date,
                });
            }
            std::cmp::Ordering::Less => break,
        }
    }

    Ok((date, count))
}

fn committer_date(commit: &gix::Commit<'_>) -> Result<String, Error> {
    let time = commit.time().map_err(git_err)?;
    epoch_to_date(time.seconds)
}

fn first_parent_id(commit: &gix::Commit<'_>) -> Option<ObjectId> {
    commit.parent_ids().next().map(gix::Id::detach)
}

struct BranchInfo {
    name: String,
    hash: ObjectId,
}

fn detect_branch(repo: &gix::Repository, override_name: Option<&str>) -> Result<BranchInfo, Error> {
    if let Some(name) = override_name {
        for ref_name in [
            format!("refs/remotes/origin/{name}"),
            format!("refs/heads/{name}"),
        ] {
            if let Some(id) = try_resolve_ref(repo, &ref_name) {
                return Ok(BranchInfo {
                    name: name.to_owned(),
                    hash: id,
                });
            }
        }
        return Err(Error::NoDefaultBranch);
    }

    // origin/HEAD symbolic ref
    if let Ok(r) = repo.find_reference("refs/remotes/origin/HEAD")
        && let Some(target_name) = r.target().try_name()
    {
        let target_str = target_name.to_string();
        let short = target_str
            .strip_prefix("refs/remotes/origin/")
            .unwrap_or(&target_str);
        if let Some(id) = try_resolve_ref(repo, &target_str) {
            return Ok(BranchInfo {
                name: short.to_owned(),
                hash: id,
            });
        }
    }

    for (prefix, name) in [
        ("refs/remotes/origin/", "main"),
        ("refs/remotes/origin/", "master"),
        ("refs/heads/", "main"),
        ("refs/heads/", "master"),
    ] {
        if let Some(id) = try_resolve_ref(repo, &format!("{prefix}{name}")) {
            return Ok(BranchInfo {
                name: name.to_owned(),
                hash: id,
            });
        }
    }

    Err(Error::NoDefaultBranch)
}

fn try_resolve_ref(repo: &gix::Repository, ref_name: &str) -> Option<ObjectId> {
    repo.find_reference(ref_name)
        .ok()?
        .peel_to_id()
        .ok()
        .map(gix::Id::detach)
}

enum BranchRelation {
    OnBranch,
    OffBranch { merge_base: ObjectId },
}

fn branch_relation(
    repo: &gix::Repository,
    target: ObjectId,
    branch: &BranchInfo,
    is_head: bool,
) -> Result<BranchRelation, Box<dyn std::error::Error + Send + Sync>> {
    if is_head && let Ok(Some(head_ref)) = repo.head_ref() {
        let head_name = head_ref.name().to_string();
        if head_name == format!("refs/heads/{}", branch.name) {
            return Ok(BranchRelation::OnBranch);
        }
    }

    if target == branch.hash {
        return Ok(BranchRelation::OnBranch);
    }

    let base = repo.merge_base(target, branch.hash)?;
    if base == target {
        Ok(BranchRelation::OnBranch)
    } else {
        Ok(BranchRelation::OffBranch {
            merge_base: base.into(),
        })
    }
}

/// Check if workspace is dirty, including untracked non-gitignored files.
/// `repo.is_dirty()` excludes untracked files; the spec requires them.
fn check_dirty(repo: &gix::Repository) -> Result<bool, Error> {
    let head_tree_id = repo.head_tree_id().map_err(git_err)?;
    let index = repo.index_or_empty().map_err(git_err)?;
    let mut index_dirty = false;
    repo.tree_index_status(
        &head_tree_id,
        &index,
        None,
        gix::status::tree_index::TrackRenames::Disabled,
        |_, _, _| {
            index_dirty = true;
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Break(()))
        },
    )
    .map_err(git_err)?;
    if index_dirty {
        return Ok(true);
    }

    if let Some(entry) = repo
        .status(gix::progress::Discard)
        .map_err(git_err)?
        .index_worktree_rewrites(None)
        .index_worktree_submodules(gix::status::Submodule::AsConfigured { check_dirty: true })
        .into_index_worktree_iter(Vec::new())
        .map_err(git_err)?
        .next()
    {
        entry.map_err(git_err)?;
        return Ok(true);
    }
    Ok(false)
}

fn short_hash(repo: &gix::Repository, id: ObjectId) -> String {
    match id.attach(repo).shorten() {
        Ok(prefix) => prefix.to_string(),
        Err(_) => id.to_string()[..7].to_owned(),
    }
}

fn epoch_to_date(seconds: gix::date::SecondsSinceUnixEpoch) -> Result<String, Error> {
    let dt = OffsetDateTime::from_unix_timestamp(seconds).map_err(git_err)?;
    Ok(format!(
        "{:04}{:02}{:02}",
        dt.year(),
        dt.month() as u8,
        dt.day()
    ))
}

fn git_err(e: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::Git(Box::new(e))
}

fn format_version(
    prefix: &str,
    date: &str,
    n: usize,
    dirty: bool,
    dirty_string: &str,
    hash: &str,
) -> String {
    let mut version = format!("{prefix}{date}.{n}");
    if dirty {
        version.push_str(dirty_string);
        if !hash.is_empty() {
            version.push('.');
            version.push_str(hash);
        }
    }
    version
}

#[must_use]
pub fn parse_version(s: &str) -> Option<(&str, usize)> {
    if s.is_empty() {
        return None;
    }

    let bytes = s.as_bytes();
    for start in 0..bytes.len() {
        if let Some(result) = try_parse_version_at(s, start) {
            return Some(result);
        }
    }
    None
}

fn try_parse_version_at(s: &str, start: usize) -> Option<(&str, usize)> {
    let s = &s[start..];
    if s.len() < 10 {
        return None;
    }

    let (date_part, rest) = s.split_at(8);
    if !date_part.bytes().all(|b| b.is_ascii_digit()) || !looks_like_date(date_part) {
        return None;
    }

    let rest = rest.strip_prefix('.')?;
    let n_str = rest.split(|c: char| !c.is_ascii_digit()).next()?;
    if n_str.is_empty() || (n_str.len() > 1 && n_str.starts_with('0')) {
        return None;
    }
    let n: usize = n_str.parse().ok()?;
    if n == 0 {
        return None;
    }

    Some((date_part, n))
}

fn looks_like_date(s: &str) -> bool {
    let year: u32 = s[0..4].parse().unwrap_or(0);
    let month: u32 = s[4..6].parse().unwrap_or(0);
    let day: u32 = s[6..8].parse().unwrap_or(0);
    year >= 1970 && (1..=12).contains(&month) && (1..=31).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(output.status.success());
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
        assert!(output.status.success());
        git_in(dir, &["rev-parse", "HEAD"])
    }

    fn new_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git_in(dir.path(), &["init", "-b", "main"]);
        dir
    }

    // Pure function tests

    #[test]
    fn looks_like_date_valid() {
        assert!(looks_like_date("19700101"));
        assert!(looks_like_date("20260412"));
        assert!(looks_like_date("20001231"));
    }

    #[test]
    fn looks_like_date_invalid() {
        assert!(!looks_like_date("19691231"));
        assert!(!looks_like_date("20261301"));
        assert!(!looks_like_date("20260032"));
        assert!(!looks_like_date("20260000"));
        assert!(!looks_like_date("00000101"));
        assert!(!looks_like_date("abcdefgh"));
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
    fn parse_version_bare() {
        assert_eq!(parse_version("20260412.1"), Some(("20260412", 1)));
        assert_eq!(parse_version("20260412.42"), Some(("20260412", 42)));
    }

    #[test]
    fn parse_version_prefixed() {
        assert_eq!(parse_version("v0.20260412.3"), Some(("20260412", 3)));
        assert_eq!(parse_version("0.20260412.1"), Some(("20260412", 1)));
    }

    #[test]
    fn parse_version_rejects_invalid() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("notaversion"), None);
        assert_eq!(parse_version("20260412.0"), None);
        assert_eq!(parse_version("20260412"), None);
        assert_eq!(parse_version("1234567.1"), None);
    }

    #[test]
    fn parse_version_non_digit_after_dot() {
        assert_eq!(parse_version("20260412.abc"), None);
    }

    #[test]
    fn parse_version_trailing_content() {
        assert_eq!(
            parse_version("20260412.5-dirty.abc1234"),
            Some(("20260412", 5)),
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
        assert!(!opts.short);
    }

    // Git-dependent tests

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
        let dir = new_repo();
        let first = commit_at(dir.path(), "2026-04-10T12:00:00Z");
        commit_at(dir.path(), "2026-04-10T13:00:00Z");

        let obj_path = dir
            .path()
            .join(".git/objects")
            .join(&first[..2])
            .join(&first[2..]);
        std::fs::remove_file(obj_path).unwrap();

        let err = run(&Options {
            dir: dir.path(),
            branch: Some("main"),
            ..Options::default()
        })
        .unwrap_err();
        assert!(matches!(err, Error::Git(_)));
    }

    #[test]
    fn short_hash_fallback() {
        let dir = new_repo();
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        let repo = gix::discover(dir.path()).unwrap();

        let bogus = gix::ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let result = short_hash(&repo, bogus);
        assert_eq!(result, "deadbee");
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
        // Version is from the merge-base (main tip = 20260410.1),
        // hash is from the feature branch commit.
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
    fn origin_head_dangling() {
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
        let result = run(&Options {
            dir: dir.path(),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(result, "20260410.1");
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
}
