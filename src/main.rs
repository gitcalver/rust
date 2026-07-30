#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::process::ExitCode;

use gitcalver::Options;

const USAGE: &str = "\
Usage: gitcalver [options] [REVISION | VERSION]
       gitcalver prepare-publish [options] --manifest PATH [--source-dir DIR]

Compute a gitcalver version for a git commit, or find the commit for a version.
Use `prepare-publish` to compute a version and patch a Cargo.toml in one shot
(intended for release flows; run `gitcalver prepare-publish --help` for details).

Options:
  --prefix PREFIX     Prepend PREFIX to version (default: empty)
  --dirty STRING      Enable dirty versions, append STRING.HASH
  --no-dirty          Refuse dirty versions (overrides --dirty)
  --no-dirty-hash     Suppress .HASH suffix (requires --dirty)
  --branch BRANCH     Override default branch detection
  --remote REMOTE     Remote used for cached branch detection (default: origin)
  --short             Output short commit hash (reverse mode only)
  --help              Show this help

Exit codes:
  0   Success
  1   Error (not a git repo, no commits, decreasing dates, etc.)
  2   Dirty workspace or off default branch (without --dirty)
  3   Cannot trace to default branch
  4   Local history is insufficient to prove the result
";

const USAGE_PREPARE_PUBLISH: &str = "\
Usage: gitcalver prepare-publish [options] --manifest PATH [--source-dir DIR]

Compute a gitcalver version for HEAD and patch the given Cargo.toml so that
`[package].version` is set to that version and `[package].publish = true`.
Comments and formatting are preserved. The computed version is printed to
stdout. Exits non-zero (and leaves the manifest unchanged) on a dirty
workspace, an off-branch HEAD, or a manifest that already sets `version` or
`publish` to a conflicting value.

Options:
  --manifest PATH     Cargo.toml to patch (required)
  --source-dir DIR    Git repo to compute version from (default: .)
  --prefix PREFIX     Prepend PREFIX to version (default: empty)
  --branch BRANCH     Override default branch detection
  --remote REMOTE     Remote used for cached branch detection (default: origin)
  --help              Show this help
";

#[cfg_attr(coverage_nightly, coverage(off))]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(cli(&args))
}

fn cli(args: &[String]) -> u8 {
    if args.first().map(String::as_str) == Some("prepare-publish") {
        return cli_prepare_publish(&args[1..]);
    }

    let parsed = match parse_args(args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => {
            print!("{USAGE}");
            return 0;
        }
        Err(msg) => {
            eprintln!("gitcalver: {msg}");
            return 1;
        }
    };

    if let Err(msg) = validate(&parsed) {
        eprintln!("gitcalver: {msg}");
        return 1;
    }

    let opts = parsed.to_options();

    match gitcalver::run(&opts) {
        Ok(result) => {
            println!("{result}");
            0
        }
        Err(e) => {
            let code = exit_code(&e);
            eprintln!("gitcalver: {e}");
            code
        }
    }
}

fn cli_prepare_publish(args: &[String]) -> u8 {
    let parsed = match parse_prepare_args(args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => {
            print!("{USAGE_PREPARE_PUBLISH}");
            return 0;
        }
        Err(msg) => {
            eprintln!("gitcalver: {msg}");
            return 1;
        }
    };

    let opts = Options {
        dir: &parsed.source_dir,
        target: None,
        prefix: &parsed.prefix,
        dirty_suffix: None,
        include_dirty_hash: true,
        branch: if parsed.branch.is_empty() {
            None
        } else {
            Some(parsed.branch.as_str())
        },
        remote: if parsed.remote.is_empty() {
            None
        } else {
            Some(parsed.remote.as_str())
        },
        short: false,
    };

    let version = match gitcalver::run(&opts) {
        Ok(v) => v,
        Err(gitcalver::Error::DirtyWorkspace) => {
            eprintln!("gitcalver: working tree is dirty; commit or stash before publishing");
            return 2;
        }
        Err(e) => {
            let code = exit_code(&e);
            eprintln!("gitcalver: {e}");
            return code;
        }
    };

    let src = match std::fs::read_to_string(&parsed.manifest) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "gitcalver: failed to read manifest at {}: {e}",
                parsed.manifest.display()
            );
            return 1;
        }
    };

    let patched = match gitcalver::patch_manifest(&src, &version) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("gitcalver: {e}");
            return 1;
        }
    };

    if let Err(e) = atomic_write(&parsed.manifest, &patched) {
        eprintln!(
            "gitcalver: failed to write manifest at {}: {e}",
            parsed.manifest.display()
        );
        return 1;
    }

    println!("{version}");
    0
}

fn atomic_write(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp_path = std::path::PathBuf::from(tmp_name);

    // Write and flush to disk before renaming so a crash cannot leave the
    // manifest renamed-but-empty; clean up the temp file if anything fails.
    let write_result = (|| {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

struct PrepareArgs {
    prefix: String,
    branch: String,
    remote: String,
    manifest: std::path::PathBuf,
    source_dir: std::path::PathBuf,
}

fn parse_prepare_args(args: &[String]) -> Result<Option<PrepareArgs>, String> {
    let mut prefix = String::new();
    let mut branch = String::new();
    let mut remote = String::new();
    let mut manifest: Option<std::path::PathBuf> = None;
    let mut source_dir: Option<std::path::PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" => return Ok(None),
            "--prefix" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "--prefix requires an argument".to_owned())?;
                prefix.clone_from(v);
            }
            "--branch" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "--branch requires an argument".to_owned())?;
                branch.clone_from(v);
            }
            "--remote" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "--remote requires an argument".to_owned())?;
                if v.is_empty() {
                    return Err("--remote requires a non-empty argument".to_owned());
                }
                remote.clone_from(v);
            }
            "--manifest" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "--manifest requires an argument".to_owned())?;
                manifest = Some(std::path::PathBuf::from(v));
            }
            "--source-dir" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "--source-dir requires an argument".to_owned())?;
                source_dir = Some(std::path::PathBuf::from(v));
            }
            _ => return Err(format!("unknown option: {arg}")),
        }
        i += 1;
    }

    let manifest = manifest.ok_or_else(|| "--manifest is required".to_owned())?;
    let source_dir = source_dir.unwrap_or_else(|| std::path::PathBuf::from("."));

    Ok(Some(PrepareArgs {
        prefix,
        branch,
        remote,
        manifest,
        source_dir,
    }))
}

const fn exit_code(err: &gitcalver::Error) -> u8 {
    match err {
        gitcalver::Error::DirtyWorkspace | gitcalver::Error::NotOnDefaultBranch { .. } => 2,
        gitcalver::Error::NotTraceable { .. } => 3,
        gitcalver::Error::IncompleteHistory(_) => 4,
        _ => 1,
    }
}

struct ParsedArgs {
    prefix: String,
    dirty_string: Option<String>,
    no_dirty: bool,
    no_dirty_hash: bool,
    branch: String,
    remote: String,
    short: bool,
    positional: String,
}

fn parse_args(args: &[String]) -> Result<Option<ParsedArgs>, String> {
    let mut parsed = ParsedArgs {
        prefix: String::new(),
        dirty_string: None,
        no_dirty: false,
        no_dirty_hash: false,
        branch: String::new(),
        remote: String::new(),
        short: false,
        positional: String::new(),
    };

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--prefix" => {
                i += 1;
                if i >= args.len() {
                    return Err("--prefix requires an argument".to_owned());
                }
                parsed.prefix.clone_from(&args[i]);
            }
            "--dirty" => {
                i += 1;
                if i >= args.len() {
                    return Err("--dirty requires an argument".to_owned());
                }
                if args[i].is_empty() {
                    return Err("--dirty requires a non-empty string".to_owned());
                }
                parsed.dirty_string = Some(args[i].clone());
            }
            "--no-dirty" => {
                parsed.no_dirty = true;
            }
            "--no-dirty-hash" => {
                parsed.no_dirty_hash = true;
            }
            "--branch" => {
                i += 1;
                if i >= args.len() {
                    return Err("--branch requires an argument".to_owned());
                }
                parsed.branch.clone_from(&args[i]);
            }
            "--remote" => {
                i += 1;
                if i >= args.len() {
                    return Err("--remote requires an argument".to_owned());
                }
                if args[i].is_empty() {
                    return Err("--remote requires a non-empty argument".to_owned());
                }
                parsed.remote.clone_from(&args[i]);
            }
            "--short" => {
                parsed.short = true;
            }
            "--help" => {
                return Ok(None);
            }
            "--" => {
                i += 1;
                if i < args.len() {
                    if !parsed.positional.is_empty() {
                        return Err(format!("unexpected argument: {}", args[i]));
                    }
                    parsed.positional.clone_from(&args[i]);
                    i += 1;
                }
                if i < args.len() {
                    return Err(format!("unexpected argument: {}", args[i]));
                }
                break;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                if !parsed.positional.is_empty() {
                    return Err(format!("unexpected argument: {arg}"));
                }
                parsed.positional.clone_from(arg);
            }
        }
        i += 1;
    }

    Ok(Some(parsed))
}

fn validate(parsed: &ParsedArgs) -> Result<(), String> {
    if parsed.no_dirty_hash && (parsed.dirty_string.is_none() || parsed.no_dirty) {
        return Err("--no-dirty-hash requires --dirty".to_owned());
    }
    if parsed.short && gitcalver::parse_version(&parsed.positional).is_none() {
        return Err("--short is only valid in reverse mode (with a version argument)".to_owned());
    }
    Ok(())
}

impl ParsedArgs {
    fn to_options(&self) -> Options<'_> {
        let dirty_suffix = if self.no_dirty {
            None
        } else {
            self.dirty_string.as_deref()
        };

        let target = if self.positional.is_empty() {
            None
        } else {
            Some(self.positional.as_str())
        };

        let branch = if self.branch.is_empty() {
            None
        } else {
            Some(self.branch.as_str())
        };

        let remote = if self.remote.is_empty() {
            None
        } else {
            Some(self.remote.as_str())
        };

        Options {
            dir: std::path::Path::new("."),
            target,
            prefix: &self.prefix,
            dirty_suffix,
            include_dirty_hash: !self.no_dirty_hash,
            branch,
            remote,
            short: self.short,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| (*s).to_owned()).collect()
    }

    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    fn cli_in_dir(dir: &std::path::Path, cli_args: &[&str]) -> u8 {
        let _lock = CWD_MUTEX.lock().unwrap();
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        let code = cli(&args(cli_args));
        std::env::set_current_dir(old).unwrap();
        code
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn git_in(dir: &std::path::Path, cli_args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(cli_args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn commit_at(dir: &std::path::Path, date: &str) {
        let output = std::process::Command::new("git")
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
    }

    fn new_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git_in(dir.path(), &["init", "-b", "main"]);
        dir
    }

    // CLI integration tests

    #[test]
    fn cli_help() {
        assert_eq!(cli(&args(&["--help"])), 0);
    }

    #[test]
    fn cli_unknown_option() {
        assert_eq!(cli(&args(&["--foo"])), 1);
    }

    #[test]
    fn cli_validate_error() {
        assert_eq!(cli(&args(&["--no-dirty-hash"])), 1);
    }

    #[test]
    fn cli_not_a_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(cli_in_dir(dir.path(), &["--branch", "main"]), 1);
    }

    #[test]
    fn cli_success() {
        let dir = new_repo();
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        assert_eq!(cli_in_dir(dir.path(), &["--branch", "main"]), 0);
    }

    #[test]
    fn cli_dirty_exit_code() {
        let dir = new_repo();
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        std::fs::write(dir.path().join("dirty.txt"), "dirty").unwrap();
        assert_eq!(cli_in_dir(dir.path(), &["--branch", "main"]), 2);
    }

    #[test]
    fn cli_wrong_branch_exit_code() {
        let dir = new_repo();
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        git_in(dir.path(), &["checkout", "-b", "feature"]);
        commit_at(dir.path(), "2026-04-10T13:00:00Z");
        assert_eq!(cli_in_dir(dir.path(), &["--branch", "main"]), 2);
    }

    #[test]
    fn cli_not_traceable_exit_code() {
        // Complete history proves the orphan target shares nothing with
        // the selected branch: exit 3, driven end-to-end through the CLI.
        let dir = new_repo();
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        git_in(dir.path(), &["checkout", "--orphan", "orphan"]);
        commit_at(dir.path(), "2026-04-10T13:00:00Z");
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let target = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        git_in(dir.path(), &["checkout", "main"]);
        assert_eq!(cli_in_dir(dir.path(), &["--branch", "main", &target]), 3);
    }

    #[test]
    fn cli_incomplete_history_exit_code() {
        // A legacy grafts file rewrites stored ancestry, so the repository
        // is rejected as incomplete: exit 4, driven end-to-end through the
        // CLI.
        let dir = new_repo();
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        let graft_dir = dir.path().join(".git/info");
        std::fs::create_dir_all(&graft_dir).unwrap();
        std::fs::write(graft_dir.join("grafts"), "").unwrap();
        assert_eq!(cli_in_dir(dir.path(), &["--branch", "main"]), 4);
    }

    #[test]
    fn cli_dirty_staged_exit_code() {
        let dir = new_repo();
        commit_at(dir.path(), "2026-04-10T12:00:00Z");
        std::fs::write(dir.path().join("staged.txt"), "staged").unwrap();
        git_in(dir.path(), &["add", "staged.txt"]);
        assert_eq!(cli_in_dir(dir.path(), &["--branch", "main"]), 2);
    }

    // parse_args tests

    #[test]
    fn parse_help() {
        assert!(matches!(parse_args(&args(&["--help"])), Ok(None)));
    }

    #[test]
    fn parse_prefix_missing() {
        assert!(parse_args(&args(&["--prefix"])).is_err());
    }

    #[test]
    fn parse_dirty_missing() {
        assert!(parse_args(&args(&["--dirty"])).is_err());
    }

    #[test]
    fn parse_dirty_empty() {
        assert!(parse_args(&args(&["--dirty", ""])).is_err());
    }

    #[test]
    fn parse_branch_missing() {
        assert!(parse_args(&args(&["--branch"])).is_err());
    }

    #[test]
    fn parse_unknown_option() {
        assert!(parse_args(&args(&["--foo"])).is_err());
    }

    #[test]
    fn parse_double_dash_positional() {
        let parsed = parse_args(&args(&["--", "--looks-like-flag"]))
            .unwrap()
            .unwrap();
        assert_eq!(parsed.positional, "--looks-like-flag");
    }

    #[test]
    fn parse_double_dash_extra_arg() {
        assert!(parse_args(&args(&["--", "abc123", "extra"])).is_err());
    }

    #[test]
    fn parse_two_positionals() {
        assert!(parse_args(&args(&["abc123", "def456"])).is_err());
    }

    #[test]
    fn parse_positional_before_double_dash_is_rejected() {
        // A positional before `--` plus one after is two positionals; reject it
        // rather than silently letting the second clobber the first.
        assert!(parse_args(&args(&["abc123", "--", "def456"])).is_err());
    }

    #[test]
    fn parse_double_dash_only() {
        // `--` with nothing after ends option parsing and leaves no positional.
        let parsed = parse_args(&args(&["--"])).unwrap().unwrap();
        assert_eq!(parsed.positional, "");
    }

    #[test]
    fn parse_all_flags() {
        let parsed = parse_args(&args(&[
            "--prefix",
            "v0.",
            "--dirty",
            "-dirty",
            "--no-dirty",
            "--no-dirty-hash",
            "--branch",
            "main",
            "--remote",
            "upstream",
            "--short",
            "abc123",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(parsed.prefix, "v0.");
        assert_eq!(parsed.dirty_string, Some("-dirty".into()));
        assert!(parsed.no_dirty);
        assert!(parsed.no_dirty_hash);
        assert_eq!(parsed.branch, "main");
        assert_eq!(parsed.remote, "upstream");
        assert!(parsed.short);
        assert_eq!(parsed.positional, "abc123");
    }

    #[test]
    fn parse_remote_missing() {
        assert!(parse_args(&args(&["--remote"])).is_err());
    }

    #[test]
    fn parse_remote_empty() {
        assert!(parse_args(&args(&["--remote", ""])).is_err());
    }

    // validate tests

    #[test]
    fn validate_ok() {
        let parsed = ParsedArgs {
            prefix: String::new(),
            dirty_string: Some("-dirty".into()),
            no_dirty: false,
            no_dirty_hash: true,
            branch: String::new(),
            remote: String::new(),
            short: false,
            positional: String::new(),
        };
        assert!(validate(&parsed).is_ok());
    }

    #[test]
    fn validate_no_dirty_hash_without_dirty() {
        let parsed = ParsedArgs {
            prefix: String::new(),
            dirty_string: None,
            no_dirty: false,
            no_dirty_hash: true,
            branch: String::new(),
            remote: String::new(),
            short: false,
            positional: String::new(),
        };
        assert!(validate(&parsed).is_err());
    }

    #[test]
    fn validate_short_without_version() {
        let parsed = ParsedArgs {
            prefix: String::new(),
            dirty_string: None,
            no_dirty: false,
            no_dirty_hash: false,
            branch: String::new(),
            remote: String::new(),
            short: true,
            positional: "not-a-version".into(),
        };
        assert!(validate(&parsed).is_err());
    }

    // to_options tests

    #[test]
    fn to_options_full() {
        let parsed = ParsedArgs {
            prefix: "v0.".into(),
            dirty_string: Some("-dirty".into()),
            no_dirty: false,
            no_dirty_hash: true,
            branch: "main".into(),
            remote: "upstream".into(),
            short: true,
            positional: "abc123".into(),
        };
        let opts = parsed.to_options();
        assert_eq!(opts.prefix, "v0.");
        assert_eq!(opts.dirty_suffix, Some("-dirty"));
        assert!(!opts.include_dirty_hash);
        assert_eq!(opts.branch, Some("main"));
        assert_eq!(opts.remote, Some("upstream"));
        assert!(opts.short);
        assert_eq!(opts.target, Some("abc123"));
    }

    #[test]
    fn to_options_no_dirty_overrides() {
        let parsed = ParsedArgs {
            prefix: String::new(),
            dirty_string: Some("-dirty".into()),
            no_dirty: true,
            no_dirty_hash: false,
            branch: String::new(),
            remote: String::new(),
            short: false,
            positional: String::new(),
        };
        let opts = parsed.to_options();
        assert!(opts.dirty_suffix.is_none());
        assert!(opts.target.is_none());
        assert!(opts.branch.is_none());
        assert!(opts.remote.is_none());
    }

    // exit_code tests

    #[test]
    fn exit_code_values() {
        assert_eq!(exit_code(&gitcalver::Error::DirtyWorkspace), 2);
        assert_eq!(
            exit_code(&gitcalver::Error::NotOnDefaultBranch {
                subject: String::new(),
                branch: String::new(),
            }),
            2
        );
        assert_eq!(
            exit_code(&gitcalver::Error::NotTraceable {
                subject: String::new(),
                branch: String::new(),
            }),
            3
        );
        assert_eq!(
            exit_code(&gitcalver::Error::IncompleteHistory(String::new())),
            4
        );
        assert_eq!(exit_code(&gitcalver::Error::NotARepository), 1);
    }

    // prepare-publish tests

    const MANIFEST_TEMPLATE: &str = "\
[package]
# Load-bearing comment.
name = \"demo\"
edition = \"2024\"
";

    fn write_manifest(dir: &std::path::Path, contents: &str) -> std::path::PathBuf {
        let path = dir.join("Cargo.toml");
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn prepare_help() {
        assert_eq!(cli(&args(&["prepare-publish", "--help"])), 0);
    }

    #[test]
    fn prepare_missing_manifest_flag() {
        assert_eq!(cli(&args(&["prepare-publish"])), 1);
    }

    #[test]
    fn prepare_unknown_option() {
        assert_eq!(
            cli(&args(&[
                "prepare-publish",
                "--manifest",
                "/tmp/x",
                "--bogus"
            ])),
            1
        );
    }

    #[test]
    fn prepare_missing_arg_value() {
        // Exercises each flag's "requires an argument" branch.
        assert_eq!(cli(&args(&["prepare-publish", "--prefix"])), 1);
        assert_eq!(cli(&args(&["prepare-publish", "--branch"])), 1);
        assert_eq!(cli(&args(&["prepare-publish", "--remote"])), 1);
        assert_eq!(cli(&args(&["prepare-publish", "--manifest"])), 1);
        assert_eq!(cli(&args(&["prepare-publish", "--source-dir"])), 1);
    }

    #[test]
    fn prepare_remote_empty_rejected() {
        assert_eq!(
            cli(&args(&[
                "prepare-publish",
                "--remote",
                "",
                "--manifest",
                "/tmp/x"
            ])),
            1
        );
    }

    #[test]
    fn prepare_source_dir_defaults_to_cwd() {
        let repo = new_repo();
        commit_at(repo.path(), "2026-04-10T12:00:00Z");
        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(staged.path(), MANIFEST_TEMPLATE);
        let manifest_str = manifest.to_str().unwrap();
        let code = cli_in_dir(
            repo.path(),
            &[
                "prepare-publish",
                "--branch",
                "main",
                "--manifest",
                manifest_str,
            ],
        );
        assert_eq!(code, 0);
        assert!(
            std::fs::read_to_string(&manifest)
                .unwrap()
                .contains("version =")
        );
    }

    #[test]
    fn prepare_happy_path() {
        let repo = new_repo();
        commit_at(repo.path(), "2026-04-10T12:00:00Z");
        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(staged.path(), MANIFEST_TEMPLATE);
        let manifest_str = manifest.to_str().unwrap();
        let source_str = repo.path().to_str().unwrap();
        let code = cli_in_dir(
            repo.path(),
            &[
                "prepare-publish",
                "--prefix",
                "0.",
                "--branch",
                "main",
                "--manifest",
                manifest_str,
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 0);
        let out = std::fs::read_to_string(&manifest).unwrap();
        assert!(out.contains("# Load-bearing comment"));
        assert!(out.contains("version = \"0.20260410.1\""));
        assert!(out.contains("publish = true"));
    }

    #[test]
    fn prepare_publish_remote_flag_threaded_through() {
        let remote_repo = new_repo();
        commit_at(remote_repo.path(), "2026-04-10T12:00:00Z");
        let parent = tempfile::tempdir().unwrap();
        git_in(
            parent.path(),
            &["clone", remote_repo.path().to_str().unwrap(), "local"],
        );
        let local = parent.path().join("local");
        git_in(&local, &["remote", "rename", "origin", "upstream"]);
        commit_at(&local, "2026-04-10T13:00:00Z");

        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(staged.path(), MANIFEST_TEMPLATE);
        let manifest_str = manifest.to_str().unwrap();
        let source_str = local.to_str().unwrap();
        let code = cli_in_dir(
            &local,
            &[
                "prepare-publish",
                "--remote",
                "upstream",
                "--manifest",
                manifest_str,
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 0);
        let out = std::fs::read_to_string(&manifest).unwrap();
        assert!(out.contains("version = \"20260410.2\""));
    }

    #[test]
    fn prepare_dirty_workspace_exit_code() {
        let repo = new_repo();
        commit_at(repo.path(), "2026-04-10T12:00:00Z");
        std::fs::write(repo.path().join("dirty.txt"), "dirty").unwrap();
        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(staged.path(), MANIFEST_TEMPLATE);
        let manifest_str = manifest.to_str().unwrap();
        let source_str = repo.path().to_str().unwrap();
        let code = cli_in_dir(
            repo.path(),
            &[
                "prepare-publish",
                "--branch",
                "main",
                "--manifest",
                manifest_str,
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 2);
        let out = std::fs::read_to_string(&manifest).unwrap();
        assert_eq!(out, MANIFEST_TEMPLATE);
    }

    #[test]
    fn prepare_other_run_error() {
        // Trigger Error::NotARepository, which goes through the generic Err arm.
        let not_a_repo = tempfile::tempdir().unwrap();
        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(staged.path(), MANIFEST_TEMPLATE);
        let manifest_str = manifest.to_str().unwrap();
        let source_str = not_a_repo.path().to_str().unwrap();
        let code = cli_in_dir(
            not_a_repo.path(),
            &[
                "prepare-publish",
                "--manifest",
                manifest_str,
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn prepare_manifest_not_readable() {
        let repo = new_repo();
        commit_at(repo.path(), "2026-04-10T12:00:00Z");
        let source_str = repo.path().to_str().unwrap();
        let code = cli_in_dir(
            repo.path(),
            &[
                "prepare-publish",
                "--branch",
                "main",
                "--manifest",
                "/no/such/path/Cargo.toml",
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn prepare_mismatched_version() {
        let repo = new_repo();
        commit_at(repo.path(), "2026-04-10T12:00:00Z");
        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(
            staged.path(),
            "[package]\nname = \"demo\"\nversion = \"9.9.9\"\n",
        );
        let manifest_str = manifest.to_str().unwrap();
        let source_str = repo.path().to_str().unwrap();
        let code = cli_in_dir(
            repo.path(),
            &[
                "prepare-publish",
                "--branch",
                "main",
                "--manifest",
                manifest_str,
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn prepare_publish_false() {
        let repo = new_repo();
        commit_at(repo.path(), "2026-04-10T12:00:00Z");
        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(
            staged.path(),
            "[package]\nname = \"demo\"\npublish = false\n",
        );
        let manifest_str = manifest.to_str().unwrap();
        let source_str = repo.path().to_str().unwrap();
        let code = cli_in_dir(
            repo.path(),
            &[
                "prepare-publish",
                "--branch",
                "main",
                "--manifest",
                manifest_str,
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn prepare_write_failure() {
        // Read + patch succeed; atomic_write fails because the `.tmp` sibling
        // path is occupied by an existing directory.
        let repo = new_repo();
        commit_at(repo.path(), "2026-04-10T12:00:00Z");
        let staged = tempfile::tempdir().unwrap();
        let manifest = write_manifest(staged.path(), MANIFEST_TEMPLATE);
        std::fs::create_dir(staged.path().join("Cargo.toml.tmp")).unwrap();
        let manifest_str = manifest.to_str().unwrap();
        let source_str = repo.path().to_str().unwrap();
        let code = cli_in_dir(
            repo.path(),
            &[
                "prepare-publish",
                "--branch",
                "main",
                "--manifest",
                manifest_str,
                "--source-dir",
                source_str,
            ],
        );
        assert_eq!(code, 1);
        assert_eq!(
            std::fs::read_to_string(&manifest).unwrap(),
            MANIFEST_TEMPLATE
        );
    }

    #[test]
    fn atomic_write_rename_failure_cleans_up_temp() {
        // The temp file writes fine, but renaming it onto an existing directory
        // fails; atomic_write must remove the temp file and propagate the error.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dest");
        std::fs::create_dir(&dest).unwrap();

        atomic_write(&dest, "contents").unwrap_err();

        assert!(
            !dir.path().join("dest.tmp").exists(),
            "temp file should be removed when the rename fails"
        );
    }
}
