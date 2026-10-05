use std::collections::{HashMap, HashSet, VecDeque};

use gix::ObjectId;
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("workspace is dirty (use --dirty to produce a dirty version)")]
    DirtyWorkspace,

    #[error("{subject} is not on the default branch ({branch})")]
    NotOnDefaultBranch { subject: String, branch: String },

    #[error("{subject} has no common history with the default branch ({branch})")]
    NotTraceable { subject: String, branch: String },

    #[error("cannot determine default branch")]
    NoDefaultBranch,

    #[error("committer dates are not non-decreasing (found {earlier} after {head} in history)")]
    DecreasingDate { head: String, earlier: String },

    #[error("no commits in repository")]
    EmptyRepository,

    #[error("not a git repository")]
    NotARepository,

    #[error("unsupported object format {0}: only SHA-1 and SHA-256 repositories are supported")]
    UnsupportedObjectFormat(String),

    #[error("--prefix must not contain a newline")]
    InvalidPrefix,

    #[error("--short is only valid in reverse lookup mode")]
    ShortRequiresReverse,

    #[error("not a gitcalver version or git revision: {0}")]
    RevisionNotFound(String),

    #[error("version {version} is missing required prefix \"{prefix}\"")]
    MissingPrefix { version: String, prefix: String },

    #[error("invalid date in version: {0}")]
    InvalidVersionDate(String),

    #[error("invalid count in version: {0}")]
    InvalidVersionCount(String),

    #[error("version not found: {0}")]
    VersionNotFound(String),

    #[error("local history cannot prove the result: {0}")]
    IncompleteHistory(String),

    #[error("{}", error_chain(.0.as_ref()))]
    Git(Box<dyn std::error::Error + Send + Sync>),
}

pub struct Options<'a> {
    pub dir: &'a std::path::Path,
    pub target: Option<&'a str>,
    pub prefix: &'a str,
    pub dirty_suffix: Option<&'a str>,
    pub include_dirty_hash: bool,
    pub branch: Option<&'a str>,
    pub remote: Option<&'a str>,
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
            remote: None,
            short: false,
        }
    }
}

/// Compute a gitcalver version string or resolve a version to a commit.
///
/// A `target` of `Some("")` is an explicit empty target and is an error, not
/// the same as `None` (HEAD).
///
/// After removing the exact configured prefix, a `target` with the complete
/// shape `YYYYMMDD.N` selects reverse mode regardless of whether the string
/// also matches a git ref, and must then carry that prefix and a valid date.
///
/// Version dates are always UTC, regardless of the committer's timezone offset.
///
/// # Errors
///
/// Returns `Error` if the directory is not a git repository, the workspace is
/// dirty (unless allowed), the target is not on the default branch, the
/// prefix, target, or flag combination is invalid, or commit history is
/// malformed or locally incomplete.
pub fn run(opts: &Options<'_>) -> Result<String, Error> {
    if opts.prefix.contains('\n') {
        return Err(Error::InvalidPrefix);
    }

    let repo = open_repo(opts.dir)?;

    if graft_file_path(&repo).is_file() {
        return Err(Error::IncompleteHistory(format!(
            "commit graft file is not supported: {}",
            graft_file_path(&repo).display()
        )));
    }

    if let Some(target) = opts.target
        && let Some((date_str, n)) = reverse_input(target, opts.prefix)?
    {
        return reverse(&repo, opts, date_str, n);
    }

    forward(&repo, opts)
}

/// Open the repository with replacement refs disabled at the object-store
/// level (so every subsequent lookup ignores them, with no process-global
/// state) rather than relying on an environment variable.
///
/// The key must be `core.useReplaceRefs`: it is the only key gix's
/// `replacement_objects_refs_prefix()` gate actually reads (the
/// `gitoxide.objects.noReplace` key is defined but never consulted in gix
/// 0.86), and an API-level override outranks both the repository's own
/// config and the `GIT_NO_REPLACE_OBJECTS` environment mapping.
///
/// gix 0.86 inverts the gate: `replacement_objects_refs_prefix()` assigns
/// the *enabled* value to a variable named `is_disabled`, so `true` is what
/// actually disables replacement honoring, and a repository setting
/// `core.useReplaceRefs=false` would otherwise enable it. Overriding to
/// `true` disables it under that inversion; the inert `replaceRefBase`
/// override keeps it disabled even if a future gix fixes the inversion and
/// starts treating `true` as enabled. The `replace_ref_ignored` test pins
/// all repository-config permutations so a gix upgrade that changes this
/// gate fails loudly.
fn open_repo(dir: &std::path::Path) -> Result<gix::Repository, Error> {
    const NO_REPLACE_OVERRIDES: [&str; 2] = [
        "core.useReplaceRefs=true",
        "gitoxide.objects.replaceRefBase=refs/gitcalver/replace-disabled/",
    ];
    let mut trust_map = gix::sec::trust::Mapping::<gix::open::Options>::default();
    trust_map.full = trust_map.full.config_overrides(NO_REPLACE_OVERRIDES);
    trust_map.reduced = trust_map.reduced.config_overrides(NO_REPLACE_OVERRIDES);

    gix::ThreadSafeRepository::discover_opts(
        dir,
        gix::discover::upwards::Options::default(),
        trust_map,
    )
    .map(|repo| repo.to_thread_local())
    .map_err(|e| {
        use gix::discover::{Error as Discover, upwards::Error as Upwards};
        match e {
            // gix reports an object format it was not built for as an
            // invalid `extensions.objectFormat` config value.
            Discover::Open(gix::open::Error::Config(gix::config::Error::ConfigTypedString(
                gix::config::key::GenericErrorWithValue {
                    key,
                    value: Some(value),
                    ..
                },
            ))) if key == "extensions.objectFormat" => {
                Error::UnsupportedObjectFormat(value.to_string())
            }
            Discover::Discover(
                Upwards::NoGitRepository { .. }
                | Upwards::NoGitRepositoryWithinCeiling { .. }
                | Upwards::NoGitRepositoryWithinFs { .. },
            ) => Error::NotARepository,
            other => git_err(other),
        }
    })
}

fn graft_file_path(repo: &gix::Repository) -> std::path::PathBuf {
    repo.common_dir().join("info/grafts")
}

fn forward(repo: &gix::Repository, opts: &Options<'_>) -> Result<String, Error> {
    if opts.short {
        return Err(Error::ShortRequiresReverse);
    }

    let is_head = opts.target.is_none();
    let subject = opts.target.unwrap_or("HEAD");

    let target_id = resolve_target(repo, subject)?;

    let remote = opts.remote.unwrap_or("origin");
    let branch_name = detect_branch_name(repo, opts.branch, remote)?;
    let branch_tip = resolve_branch_tip(repo, &branch_name, remote)?;

    let anchor = locate_anchor(repo, target_id, branch_tip, subject, &branch_name)?;

    let (version_commit, off_branch) = match anchor {
        Anchor::OnChain => (target_id, false),
        Anchor::OffChain(anchor_id) => (anchor_id, true),
    };

    let workspace_dirty = if is_head && repo.workdir().is_some() {
        check_dirty(repo)?
    } else {
        false
    };
    let dirty = workspace_dirty || off_branch;

    if dirty && opts.dirty_suffix.is_none() {
        return if off_branch {
            Err(Error::NotOnDefaultBranch {
                subject: subject.to_owned(),
                branch: branch_name,
            })
        } else {
            Err(Error::DirtyWorkspace)
        };
    }

    let (date, count) = cohort(repo, version_commit)?;

    let hash = if dirty && opts.include_dirty_hash {
        short_hash(target_id)
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

/// `HEAD` is resolved by reading refs only, never an object: an unborn `HEAD`
/// is `EmptyRepository`, while a `HEAD` whose commit is absent locally is
/// incomplete history. gix's revision parser peels symbolic refs through the
/// object store and reports a missing target as a malformed spec, so a name it
/// rejects is read as a full-length object ID or as a ref. Either is
/// well-formed whether or not its object is present, and goes on to be peeled,
/// which reports an absent object as incomplete history.
fn resolve_target(repo: &gix::Repository, subject: &str) -> Result<ObjectId, Error> {
    let id = if subject == "HEAD" {
        head_id(repo)?
    } else {
        match repo.rev_parse_single(expand_short_id(repo, subject)?.as_ref()) {
            Ok(id) => id.detach(),
            Err(_) => ObjectId::from_hex(subject.as_bytes())
                .ok()
                .filter(|id| id.kind() == repo.object_hash())
                .or_else(|| try_resolve_ref(repo, subject))
                .ok_or_else(|| Error::RevisionNotFound(subject.to_owned()))?,
        }
    };
    peel_to_commit(repo, id, subject)
}

/// gix parses a short hexadecimal ID as SHA-1 even in a SHA-256 repository, so
/// there the leading hex run is expanded to a full ID before parsing. A ref of
/// that name still wins, as in git. Only `^`, `~` and `:` can follow an object
/// ID, and none of them is legal in a ref name; `@` is, so a name such as
/// `cafe@work` is left to the parser.
fn expand_short_id<'a>(
    repo: &gix::Repository,
    rev: &'a str,
) -> Result<std::borrow::Cow<'a, str>, Error> {
    if repo.object_hash() != gix::hash::Kind::Sha256 {
        return Ok(std::borrow::Cow::Borrowed(rev));
    }

    let hex_len = rev.bytes().take_while(u8::is_ascii_hexdigit).count();
    let (hex, suffix) = rev.split_at(hex_len);
    let hash_len = repo.object_hash().len_in_hex();
    if !(gix::hash::Prefix::MIN_HEX_LEN..hash_len).contains(&hex_len)
        || !(suffix.is_empty() || suffix.starts_with(['^', '~', ':']))
        || repo.try_find_reference(hex).map_err(git_err)?.is_some()
    {
        return Ok(std::borrow::Cow::Borrowed(rev));
    }

    let padded = format!("{hex:0<hash_len$}");
    let id = ObjectId::from_hex(padded.as_bytes()).expect("validated hexadecimal prefix");
    let prefix = gix::hash::Prefix::new(&id, hex_len).expect("validated prefix length");
    let Some(Ok(full)) = repo.objects.lookup_prefix(prefix, None).map_err(git_err)? else {
        return Err(Error::RevisionNotFound(rev.to_owned()));
    };
    Ok(std::borrow::Cow::Owned(format!("{full}{suffix}")))
}

/// The object `HEAD` names. gix's `Head::id` stops at the first symbolic ref, so
/// a branch that is itself an alias has to be followed here.
fn head_id(repo: &gix::Repository) -> Result<ObjectId, Error> {
    use gix::head::Kind;
    use gix::prelude::ReferenceExt as _;

    match repo.head().map_err(git_err)?.kind {
        Kind::Unborn(_) => Err(Error::EmptyRepository),
        Kind::Detached { target, peeled } => Ok(peeled.unwrap_or(target)),
        Kind::Symbolic(referent) => {
            follow_to_id(referent.attach(repo)).ok_or(Error::EmptyRepository)
        }
    }
}

/// A tree or blob, even behind tags, is not a revision this tool can version.
/// gix reads loose objects without checking their hashes, so a corrupt store
/// can hold a tag chain that loops; a tag cycle is not a revision either.
fn peel_to_commit(
    repo: &gix::Repository,
    mut id: ObjectId,
    subject: &str,
) -> Result<ObjectId, Error> {
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(id) {
            return Err(Error::RevisionNotFound(subject.to_owned()));
        }
        let object = repo.try_find_object(id).map_err(git_err)?.ok_or_else(|| {
            Error::IncompleteHistory(format!("{subject} is missing from local history"))
        })?;
        match object.kind {
            gix::object::Kind::Commit => return Ok(id),
            gix::object::Kind::Tag => {
                id = object.to_tag_ref_iter().target_id().map_err(git_err)?;
            }
            gix::object::Kind::Tree | gix::object::Kind::Blob => {
                return Err(Error::RevisionNotFound(subject.to_owned()));
            }
        }
    }
}

fn reverse(
    repo: &gix::Repository,
    opts: &Options<'_>,
    date_str: &str,
    n: usize,
) -> Result<String, Error> {
    let remote = opts.remote.unwrap_or("origin");
    let branch_name = detect_branch_name(repo, opts.branch, remote)?;
    let branch_tip = resolve_branch_tip(repo, &branch_name, remote)?;

    let mut members = date_block_members(repo, branch_tip, date_str)?;
    // `members` is newest-first from the chain walk; cohort size increases
    // strictly oldest-to-newest within a block, so search that direction to
    // exploit the early-stop optimization.
    members.reverse();

    for member in members {
        let (_, count) = cohort(repo, member)?;
        match count.cmp(&n) {
            std::cmp::Ordering::Equal => {
                return Ok(if opts.short {
                    short_hash(member)
                } else {
                    member.to_string()
                });
            }
            std::cmp::Ordering::Greater => break,
            std::cmp::Ordering::Less => {}
        }
    }

    Err(Error::VersionNotFound(
        opts.target.unwrap_or_default().to_owned(),
    ))
}

/// Compute the 0.3 same-date cohort: the set of commits reachable from
/// `start` through any parent whose UTC committer date equals its own.
/// Traversal prunes at the first older-dated commit on each path and fails
/// fast on a newer-dated one or a commit that cannot be loaded, because the
/// count is defined as the *size* of the whole cohort — one unprovable
/// same-date parent makes the count itself unprovable.
fn cohort(repo: &gix::Repository, start: ObjectId) -> Result<(String, usize), Error> {
    let start_commit = repo.find_commit(start).map_err(git_err)?;
    let date = committer_date(&start_commit)?;
    let shallow = shallow_cuts(repo)?;

    let mut visited = HashSet::new();
    visited.insert(start);
    let mut queue = VecDeque::new();
    queue.push_back(start_commit);
    let mut count = 0usize;

    while let Some(commit) = queue.pop_front() {
        count += 1;
        // A counted commit recorded as a shallow boundary hides its true
        // ancestry even when its stored parents' objects happen to be
        // present through another path: git's traversal grafts end there,
        // so the reference implementation cannot see past it, and every
        // implementation must agree. A boundary-marked true root (real
        // depth-limited clones list those too) hides nothing.
        if commit.parent_ids().next().is_some() && shallow.contains(&commit.id().detach()) {
            return Err(Error::IncompleteHistory(format!(
                "local history ended inside the {date} date block"
            )));
        }
        for parent in commit.parent_ids() {
            let parent_id = parent.detach();
            if !visited.insert(parent_id) {
                continue;
            }
            let parent_commit = repo.find_commit(parent_id).map_err(|_| {
                Error::IncompleteHistory(format!(
                    "local history ended inside the {date} date block"
                ))
            })?;
            let parent_date = committer_date(&parent_commit)?;
            match parent_date.as_str().cmp(date.as_str()) {
                std::cmp::Ordering::Equal => queue.push_back(parent_commit),
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Greater => {
                    return Err(Error::DecreasingDate {
                        head: date,
                        earlier: parent_date,
                    });
                }
            }
        }
    }

    Ok((date, count))
}

/// Walk the selected chain's first-parent history collecting members whose
/// date equals `target_date`, proving either a strictly-older boundary
/// commit or a true root. Returns members newest-first. A same-date run that
/// cannot be extended because the next commit is missing is incomplete
/// history, not a proven boundary — unlike a plain `break`, which would
/// silently treat that as if the boundary were proven.
fn date_block_members(
    repo: &gix::Repository,
    chain_tip: ObjectId,
    target_date: &str,
) -> Result<Vec<ObjectId>, Error> {
    let mut members = Vec::new();
    let mut current = chain_tip;
    let mut prev_date: Option<String> = None;
    let shallow = shallow_cuts(repo)?;

    loop {
        let commit = repo.find_commit(current).map_err(|_| {
            Error::IncompleteHistory(
                "local history ended before the date block's boundary could be proved".to_owned(),
            )
        })?;
        let commit_date = committer_date(&commit)?;

        if let Some(ref prev) = prev_date
            && commit_date.as_str() > prev.as_str()
        {
            return Err(Error::DecreasingDate {
                head: prev.clone(),
                earlier: commit_date,
            });
        }

        if commit_date == target_date {
            members.push(current);
        } else if commit_date.as_str() < target_date {
            return Ok(members);
        }

        prev_date = Some(commit_date);

        // Traversal ends at a shallow boundary even when the stored
        // parent's object is present through another path; since the
        // strictly-older boundary has not been proved yet at this point,
        // the block cannot be proved either.
        if first_parent_id(&commit).is_some() && shallow.contains(&current) {
            return Err(Error::IncompleteHistory(
                "local history ended before the date block's boundary could be proved".to_owned(),
            ));
        }

        match first_parent_id(&commit) {
            Some(parent) => current = parent,
            None => return Ok(members),
        }
    }
}

/// The commits recorded as shallow-clone cuts in `.git/shallow`.
fn shallow_cuts(repo: &gix::Repository) -> Result<HashSet<ObjectId>, Error> {
    Ok(repo
        .shallow_commits()
        .map_err(git_err)?
        .map(|cuts| cuts.iter().copied().collect())
        .unwrap_or_default())
}

fn committer_date(commit: &gix::Commit<'_>) -> Result<String, Error> {
    let time = commit.time().map_err(git_err)?;
    epoch_to_date(time.seconds)
}

fn first_parent_id(commit: &gix::Commit<'_>) -> Option<ObjectId> {
    commit.parent_ids().next().map(gix::Id::detach)
}

/// A branch's first-parent chain, newest to oldest, with an index for O(1)
/// membership checks. `incomplete` records whether the walk stopped at a
/// load failure rather than a true root.
struct Chain {
    members: Vec<ObjectId>,
    index: HashMap<ObjectId, usize>,
    incomplete: bool,
}

fn build_chain(repo: &gix::Repository, tip: ObjectId) -> Chain {
    let mut members = Vec::new();
    let mut index = HashMap::new();
    let mut incomplete = false;
    let mut current = tip;

    loop {
        if let Ok(commit) = repo.find_commit(current) {
            index.insert(current, members.len());
            members.push(current);
            match first_parent_id(&commit) {
                Some(parent) => current = parent,
                None => break,
            }
        } else {
            incomplete = true;
            break;
        }
    }

    Chain {
        members,
        index,
        incomplete,
    }
}

enum Anchor {
    OnChain,
    OffChain(ObjectId),
}

/// Locate the newest selected-chain commit reachable from `target` through
/// any parent. `target` is enqueued first, so a `target` that is itself a
/// chain member (at any position, not just the tip) is found immediately and
/// classified `OnChain`; this makes on-chain and off-chain the same code
/// path.
///
/// A chain hit prunes that path (everything past it on the chain is only
/// ever older, never a better answer) but does not stop the search: other
/// paths may still reach a newer chain member, and the spec requires the
/// *newest* one, not merely the first one discovered by traversal order. A
/// load failure marks the search incomplete without aborting it, matching
/// cohort counting's opposite fail-fast rule: here any successful
/// intersection anywhere answers the question, so only the failure to find
/// *any* answer is disqualifying.
fn locate_anchor(
    repo: &gix::Repository,
    target: ObjectId,
    branch_tip: ObjectId,
    subject: &str,
    branch_name: &str,
) -> Result<Anchor, Error> {
    let chain = build_chain(repo, branch_tip);

    let mut visited = HashSet::new();
    visited.insert(target);
    let mut queue = VecDeque::new();
    queue.push_back(target);

    let mut best: Option<usize> = None;
    // The target walk and the chain walk fail differently: a truncated
    // chain only hides members older than everything indexed, so an anchor
    // found in the indexed part is still the newest, while an unreadable
    // commit in the target's own ancestry leaves paths unexplored that
    // could reach a newer chain member — then any anchor found elsewhere is
    // unproven and the result must be incomplete history, not a guess.
    let mut walk_incomplete = false;

    while let Some(id) = queue.pop_front() {
        if let Some(&idx) = chain.index.get(&id) {
            best = Some(best.map_or(idx, |current_best| current_best.min(idx)));
            continue;
        }
        match repo.find_commit(id) {
            Ok(commit) => {
                for parent in commit.parent_ids() {
                    let parent_id = parent.detach();
                    if visited.insert(parent_id) {
                        queue.push_back(parent_id);
                    }
                }
            }
            Err(_) => walk_incomplete = true,
        }
    }

    match best {
        Some(idx) if chain.members[idx] == target => Ok(Anchor::OnChain),
        Some(_) if walk_incomplete => Err(Error::IncompleteHistory(format!(
            "local history cannot prove {subject}'s newest reachable anchor on the default branch ({branch_name})"
        ))),
        Some(idx) => Ok(Anchor::OffChain(chain.members[idx])),
        None if walk_incomplete || chain.incomplete => Err(Error::IncompleteHistory(format!(
            "local history cannot prove {subject}'s relationship to the default branch ({branch_name})"
        ))),
        None => Err(Error::NotTraceable {
            subject: subject.to_owned(),
            branch: branch_name.to_owned(),
        }),
    }
}

/// Determine the selected branch's name. Does not resolve a tip; see
/// `resolve_branch_tip`, which is applied uniformly afterward so a name
/// selected via a remote-tracking tier still prefers its local branch.
fn detect_branch_name(
    repo: &gix::Repository,
    override_name: Option<&str>,
    remote: &str,
) -> Result<String, Error> {
    if let Some(name) = override_name {
        return Ok(name.to_owned());
    }

    let remote_prefix = format!("refs/remotes/{remote}/");
    let symbolic_head = format!("{remote_prefix}HEAD");
    if let Ok(r) = repo.find_reference(symbolic_head.as_str())
        && let Some(target_name) = r.target().try_name()
        && let Some(short) = target_name
            .as_bstr()
            .to_string()
            .strip_prefix(&remote_prefix)
    {
        return Ok(short.to_owned());
    }

    for prefix in [remote_prefix.as_str(), "refs/heads/"] {
        for name in ["main", "master"] {
            if try_resolve_ref(repo, &format!("{prefix}{name}")).is_some() {
                return Ok(name.to_owned());
            }
        }
    }

    Err(Error::NoDefaultBranch)
}

/// Resolve a selected branch name to its tip commit, preferring the local
/// branch so clean, unpushed commits remain calculable.
fn resolve_branch_tip(repo: &gix::Repository, name: &str, remote: &str) -> Result<ObjectId, Error> {
    try_resolve_ref(repo, &format!("refs/heads/{name}"))
        .or_else(|| try_resolve_ref(repo, &format!("refs/remotes/{remote}/{name}")))
        .ok_or(Error::NoDefaultBranch)
}

/// Resolve a ref to its direct target object ID, purely by reading the ref
/// (following any symbolic indirection), without loading or peeling the
/// target object itself. Branch refs always point directly at a commit, so
/// this never needs object-store access -- unlike `peel_to_id`, which reads
/// the target object to check whether it is a tag needing further peeling,
/// and so would misreport a resolvable ref as unresolvable if that object
/// happened to be locally missing.
fn try_resolve_ref(repo: &gix::Repository, ref_name: &str) -> Option<ObjectId> {
    follow_to_id(repo.find_reference(ref_name).ok()?)
}

/// `None` when a link in the chain dangles, or the chain is cyclic or longer
/// than git itself follows.
fn follow_to_id(mut reference: gix::Reference<'_>) -> Option<ObjectId> {
    reference.follow_to_object().ok().map(gix::Id::detach)
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

/// The dirty hash is always the literal first seven characters of the full
/// object ID. Spec: "MUST NOT use git's repository-dependent abbreviation
/// machinery" (which `core.abbrev` and ambiguity-based shortening are).
fn short_hash(id: ObjectId) -> String {
    id.to_string()[..7].to_owned()
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

/// An error and its sources as `outer: inner`; gix reports the actionable
/// reason, such as a bad config line, only in a source.
fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
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

/// Classify `target` as a reverse-lookup input, returning its date and count.
///
/// Only a complete `YYYYMMDD.N` left after removing the exact `prefix` enters
/// reverse mode; trailing or leading text means it is a revision instead. Once
/// a complete shape is seen the input is committed to reverse mode even if it
/// is unusable, so a tag named like a version can never shadow the lookup.
fn reverse_input<'a>(target: &'a str, prefix: &str) -> Result<Option<(&'a str, usize)>, Error> {
    let stripped = target.strip_prefix(prefix);
    let Some((date, count)) = split_version(stripped.unwrap_or(target)) else {
        return Ok(None);
    };

    if stripped.is_none() {
        return Err(Error::MissingPrefix {
            version: target.to_owned(),
            prefix: prefix.to_owned(),
        });
    }
    if !is_gregorian_date(date) {
        return Err(Error::InvalidVersionDate(target.to_owned()));
    }
    let n = count
        .parse()
        .map_err(|_| Error::InvalidVersionCount(target.to_owned()))?;
    Ok(Some((date, n)))
}

/// Split exactly `YYYYMMDD.N` (ASCII digits, no leading zero in `N`) without
/// validating the date.
fn split_version(s: &str) -> Option<(&str, &str)> {
    let (date, count) = s.split_once('.')?;
    let all_digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    (date.len() == 8
        && all_digits(date)
        && !count.is_empty()
        && all_digits(count)
        && !count.starts_with('0'))
    .then_some((date, count))
}

/// `date` must be eight ASCII digits.
fn is_gregorian_date(date: &str) -> bool {
    let year = date[..4].parse::<i32>();
    let month = date[4..6]
        .parse::<u8>()
        .ok()
        .and_then(|m| time::Month::try_from(m).ok());
    let day = date[6..].parse::<u8>();
    matches!(
        (year, month, day),
        (Ok(y), Some(m), Ok(d)) if y >= 1 && time::Date::from_calendar_date(y, m, d).is_ok()
    )
}

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("invalid TOML: {0}")]
    Parse(Box<toml_edit::TomlError>),

    #[error("manifest has no `[package]` section")]
    MissingPackage,

    #[error("workspace version inheritance not supported; set version in package manifest")]
    WorkspaceInheritance,

    #[error(
        "manifest version `{existing}` differs from computed `{computed}`; refusing to overwrite"
    )]
    VersionMismatch { existing: String, computed: String },

    #[error("manifest sets `publish = false`; refusing to overwrite")]
    PublishFalse,
}

impl From<toml_edit::TomlError> for PatchError {
    fn from(e: toml_edit::TomlError) -> Self {
        Self::Parse(Box::new(e))
    }
}

/// Patch a Cargo manifest by setting `[package].version` and `[package].publish = true`,
/// preserving comments and formatting.
///
/// If `version` or `publish` are already set to matching values, the operation is
/// idempotent. Mismatched values cause an error rather than overwriting, protecting
/// against accidentally pointing at the source manifest instead of a staged copy.
///
/// # Errors
///
/// Returns `PatchError` for malformed TOML, a missing `[package]` table, workspace
/// version inheritance, or a manifest that already sets `version`/`publish` to a
/// value that conflicts with the desired one.
pub fn patch_manifest(toml_src: &str, version: &str) -> Result<String, PatchError> {
    let mut doc: toml_edit::DocumentMut = toml_src.parse()?;

    let package = doc
        .get_mut("package")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or(PatchError::MissingPackage)?;

    match package.get("version") {
        None => {
            package.insert("version", toml_edit::value(version));
        }
        Some(item) => match item.as_value().and_then(toml_edit::Value::as_str) {
            Some(existing) if existing == version => {}
            Some(existing) => {
                return Err(PatchError::VersionMismatch {
                    existing: existing.to_owned(),
                    computed: version.to_owned(),
                });
            }
            None => return Err(PatchError::WorkspaceInheritance),
        },
    }

    match package.get("publish") {
        None => {
            package.insert("publish", toml_edit::value(true));
        }
        Some(item) => match item.as_value() {
            Some(toml_edit::Value::Boolean(b)) if *b.value() => {}
            Some(toml_edit::Value::Boolean(_)) => return Err(PatchError::PublishFalse),
            // `publish = ["registry", ...]` restricts publishing to the listed
            // registries; treat as already configured and leave untouched.
            Some(toml_edit::Value::Array(_)) => {}
            _ => return Err(PatchError::WorkspaceInheritance),
        },
    }

    Ok(doc.to_string())
}

#[cfg(test)]
mod tests;
