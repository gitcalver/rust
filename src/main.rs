#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::process::ExitCode;

use gitcalver::Options;

const USAGE: &str = "\
Usage: gitcalver [options] [REVISION | VERSION]

Compute a gitcalver version for a git commit, or find the commit for a version.

Options:
  --prefix PREFIX     Prepend PREFIX to version (default: empty)
  --dirty STRING      Enable dirty versions, append STRING.HASH
  --no-dirty          Refuse dirty versions (overrides --dirty)
  --no-dirty-hash     Suppress .HASH suffix (requires --dirty)
  --branch BRANCH     Override default branch detection
  --short             Output short commit hash (reverse mode only)
  --help              Show this help
";

#[cfg_attr(coverage_nightly, coverage(off))]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(cli(&args))
}

fn cli(args: &[String]) -> u8 {
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

const fn exit_code(err: &gitcalver::Error) -> u8 {
    match err {
        gitcalver::Error::DirtyWorkspace | gitcalver::Error::NotOnDefaultBranch { .. } => 2,
        gitcalver::Error::NotTraceable { .. } => 3,
        _ => 1,
    }
}

struct ParsedArgs {
    prefix: String,
    dirty_string: Option<String>,
    no_dirty: bool,
    no_dirty_hash: bool,
    branch: String,
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
            "--short" => {
                parsed.short = true;
            }
            "--help" => {
                return Ok(None);
            }
            "--" => {
                i += 1;
                if i < args.len() {
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

        Options {
            dir: std::path::Path::new("."),
            target,
            prefix: &self.prefix,
            dirty_suffix,
            include_dirty_hash: !self.no_dirty_hash,
            branch,
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
        assert!(parsed.short);
        assert_eq!(parsed.positional, "abc123");
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
            short: true,
            positional: "abc123".into(),
        };
        let opts = parsed.to_options();
        assert_eq!(opts.prefix, "v0.");
        assert_eq!(opts.dirty_suffix, Some("-dirty"));
        assert!(!opts.include_dirty_hash);
        assert_eq!(opts.branch, Some("main"));
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
            short: false,
            positional: String::new(),
        };
        let opts = parsed.to_options();
        assert!(opts.dirty_suffix.is_none());
        assert!(opts.target.is_none());
        assert!(opts.branch.is_none());
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
                source: "test".into(),
            }),
            3
        );
        assert_eq!(exit_code(&gitcalver::Error::NotARepository), 1);
    }
}
