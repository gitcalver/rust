//! Repository discovery driven by git's environment variables. They are set on
//! a child process because the crate forbids the `unsafe` needed to set them in
//! this one.

use std::path::Path;
use std::process::{Command, Output};

const GIT_ENVIRONMENT: [&str; 4] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

fn git(dir: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .env("GIT_AUTHOR_DATE", "2026-04-10T12:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-04-10T12:00:00Z");
    for name in GIT_ENVIRONMENT {
        command.env_remove(name);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository whose version at HEAD is `20260410.<commits>`.
fn repo_with_commits(dir: &Path, commits: usize) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-b", "main"]);
    for _ in 0..commits {
        git(
            dir,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "test",
            ],
        );
    }
}

/// Run gitcalver on `HEAD`, which ignores the workspace, so the extra
/// directories these tests create cannot make the result dirty.
fn gitcalver(dir: &Path, environment: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gitcalver"));
    command.args(["--branch", "main", "HEAD"]).current_dir(dir);
    for name in GIT_ENVIRONMENT {
        command.env_remove(name);
    }
    for (name, value) in environment {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .trim()
        .to_owned()
}

fn assert_not_a_repository(output: &Output) {
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.trim_end(), "gitcalver: not a git repository");
}

#[test]
fn git_dir_selects_the_repository() {
    let root = tempfile::tempdir().unwrap();
    let here = root.path().join("here");
    let elsewhere = root.path().join("elsewhere");
    repo_with_commits(&here, 1);
    repo_with_commits(&elsewhere, 2);

    assert_eq!(stdout(&gitcalver(&here, &[])), "20260410.1");
    let git_dir = elsewhere.join(".git");
    assert_eq!(
        stdout(&gitcalver(&here, &[("GIT_DIR", &git_dir)])),
        "20260410.2"
    );
    // Ceilings play no part when GIT_DIR names the repository.
    assert_eq!(
        stdout(&gitcalver(
            &here,
            &[
                ("GIT_DIR", &git_dir),
                ("GIT_CEILING_DIRECTORIES", root.path())
            ]
        )),
        "20260410.2"
    );
}

#[test]
fn git_dir_that_is_not_a_repository_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    let here = root.path().join("here");
    repo_with_commits(&here, 1);

    let missing = root.path().join("missing");
    assert_not_a_repository(&gitcalver(&here, &[("GIT_DIR", &missing)]));
    // An empty value is set, not absent, so there is no fallback to discovery.
    assert_not_a_repository(&gitcalver(&here, &[("GIT_DIR", Path::new(""))]));
}

#[test]
fn ceiling_directories_are_read_from_the_environment() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().canonicalize().unwrap().join("repo");
    repo_with_commits(&repo, 1);
    let nested = repo.join("sub");
    std::fs::create_dir_all(&nested).unwrap();

    assert_not_a_repository(&gitcalver(&nested, &[("GIT_CEILING_DIRECTORIES", &repo)]));
    // A ceiling between the start directory and the repository ends the search
    // before it finds one.
    let deeper = nested.join("deeper");
    std::fs::create_dir_all(&deeper).unwrap();
    assert_not_a_repository(&gitcalver(&deeper, &[("GIT_CEILING_DIRECTORIES", &nested)]));
    let above = repo.parent().unwrap();
    assert_eq!(
        stdout(&gitcalver(&nested, &[("GIT_CEILING_DIRECTORIES", above)])),
        "20260410.1"
    );
    // As in git, a ceiling that does not contain the start directory is ignored.
    assert_eq!(
        stdout(&gitcalver(
            &nested,
            &[("GIT_CEILING_DIRECTORIES", Path::new("/nonexistent"))]
        )),
        "20260410.1"
    );
}
