# gitcalver

[![docs.rs](https://img.shields.io/docsrs/gitcalver)](https://docs.rs/gitcalver)

A Rust implementation of [GitCalVer](https://gitcalver.org), which derives
calendar-based version numbers from git history.

Each commit on the default branch gets a unique, strictly increasing version of
the form `YYYYMMDD.N`, where `N` is the size of that commit's date cohort: the
commits reachable from it through any parent whose UTC committer date is the
same. Counting through every parent (not just the first) is what keeps
versions strictly increasing even across merges that reparent the default
branch's history.

Because a merge can pull same-date commits in from another branch, `N` can
jump by more than one from one default-branch commit to the next; the
sequence is strictly increasing but not necessarily contiguous. Reverse lookup
for a skipped value reports "version not found".

See the [GitCalVer specification](https://gitcalver.org) for full details.

## Installation

Git history is read directly using [gix](https://github.com/GitoxideLabs/gitoxide);
no external `git` binary is required at runtime.

```sh
cargo install gitcalver
```

Or build from source:

```sh
make build
```

## Usage

```
gitcalver [OPTIONS] [REVISION | VERSION]
```

With no arguments, outputs the version for HEAD:

```sh
$ gitcalver
20260411.3
```

An omitted target checks the workspace. An explicit revision, including
`HEAD`, calculates that commit's version without considering workspace
changes. Bare repositories support explicit revisions, reverse lookup, and an
omitted target without attempting a workspace check.

### Version prefix

Use `--prefix` to prepend a string to the version number, e.g.:

| Use case | Command                    | Example output  |
| -------- | --------------------------- | ---------------- |
| Default  | `gitcalver`                 | `20260411.3`     |
| SemVer   | `gitcalver --prefix "0."`   | `0.20260411.3`   |
| Cargo    | `gitcalver --prefix "0."`   | `0.20260411.3`   |

### Dirty workspace

By default, gitcalver exits with status 2 if the workspace has uncommitted
changes. Use `--dirty STRING` to produce a version instead; the output will
include the given string and a short commit hash (e.g. `--dirty "-dirty"`
produces `20260411.3-dirty.abc1234`). The hash is always the literal first
seven characters of the commit's full object ID, never git's
repository-dependent abbreviation.

Use `--no-dirty-hash` with `--dirty` to suppress the hash suffix.
Use `--no-dirty` to explicitly refuse dirty versions (overrides `--dirty`).

Dirty versions are a convenience and are not necessarily unique.

### Reverse lookup

Pass a version number instead of a revision to get the corresponding commit
hash:

```sh
$ gitcalver 20260411.3
a1b2c3d4e5f6...

$ gitcalver --short --prefix "0." 0.20260411.3
a1b2c3d
```

If the version was generated with `--prefix`, pass the same `--prefix` for
reverse lookup.

Dirty versions cannot be reversed.

### Options

| Option             | Description                                                        |
| ------------------- | -------------------------------------------------------------------- |
| `--prefix PREFIX`  | Literal string prepended to version                                |
| `--dirty STRING`   | Enable dirty versions; append `STRING.HASH`                        |
| `--no-dirty`       | Refuse dirty versions (overrides `--dirty`)                        |
| `--no-dirty-hash`  | Suppress `.HASH` suffix (requires `--dirty`)                       |
| `--branch BRANCH`  | Override default branch detection                                  |
| `--remote REMOTE`  | Remote used for cached branch detection (default: `origin`); never fetches |
| `--short`          | Output first seven object-ID characters (reverse mode only)        |
| `--help`           | Show help                                                           |

### Exit codes

| Code | Meaning                                                       |
| ---- | -------------------------------------------------------------- |
| 0    | Success                                                        |
| 1    | Error (not a git repo, no commits, decreasing dates, etc.)     |
| 2    | Dirty workspace or off default branch (without `--dirty`)      |
| 3    | Cannot trace to default branch                                 |
| 4    | Local history is insufficient to prove the result              |

## History requirements

Calculation and reverse lookup are always offline: gix is compiled here
without any network-transport feature, so there is no code path that could
perform a lazy fetch. Shallow and partial clones succeed when their local
commit objects prove the target's selected-branch relationship (or reachable
anchor) and the complete relevant date cohort, or a true repository root.
Missing required commits produce exit code 4, not a guessed version. Legacy
`info/grafts` files and replacement refs are rejected and ignored,
respectively, for the same reason: both can silently substitute ancestry that
isn't actually present.

## `prepare-publish`

`gitcalver prepare-publish` computes a version for `HEAD` and patches a
`Cargo.toml`'s `[package].version` and `[package].publish = true` in one
step, preserving comments and formatting. It is intended for release
automation; run `gitcalver prepare-publish --help` for its options
(`--manifest`, `--source-dir`, `--prefix`, `--branch`, `--remote`).

Unlike sh's `action/publish.sh`, this crate does not implement a remote-tag
race-protection protocol before publishing: crates.io itself rejects a
duplicate version atomically, and `cargo publish` always runs against a fresh
checkout, so most of that protocol would be redundant here. The one residual
gap this leaves is that a history rewrite producing a version *lower* than an
already-published one is not caught locally.
