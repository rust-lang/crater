use crate::db::{Database, QueryUtils};
use crate::prelude::*;
use crate::server::github::GitHub;
use regex::Regex;

lazy_static! {
    static ref HOMU_COMMENT_RE: Regex = Regex::new(r"<!-- homu: (\{.*\}) -->").unwrap();
}

#[derive(serde_derive::Deserialize)]
#[serde(tag = "type")]
enum HomuComment {
    TryBuildCompleted { merge_sha: String },
}

pub(crate) struct TryBuild {
    pub(crate) base_sha: String,
    pub(crate) merge_sha: String,
}

fn base_commit(gh: &dyn GitHub, repo: &str, merge_sha: &str) -> Fallible<Option<String>> {
    let mut commit = gh.get_commit(repo, &merge_sha)?;
    if commit.parents.len() != 2 {
        return Ok(None);
    }
    Ok(Some(commit.parents.remove(0).sha))
}

pub(crate) fn detect(
    db: &Database,
    gh: &dyn GitHub,
    repo: &str,
    pr: i32,
    comment: &str,
) -> Fallible<()> {
    if let Some(HomuComment::TryBuildCompleted { merge_sha }) = HOMU_COMMENT_RE
        .captures(comment)
        .and_then(|captures| serde_json::from_str(&captures[1]).ok())
    {
        if let Some(base_sha) = base_commit(gh, repo, &merge_sha)? {
            db.execute(
                "INSERT OR REPLACE INTO try_builds (repo, pr, base_sha, merge_sha) \
                 VALUES (?1, ?2, ?3, ?4);",
                &[&repo, &pr, &base_sha, &merge_sha],
            )?;
        }
    }
    Ok(())
}

pub(crate) fn get_sha(db: &Database, repo: &str, pr: i32) -> Fallible<Option<TryBuild>> {
    db.get_row(
        "SELECT base_sha, merge_sha FROM try_builds WHERE repo = ?1 AND pr = ?2;",
        &[&repo, &pr],
        |row| TryBuild {
            base_sha: row.get("base_sha"),
            merge_sha: row.get("merge_sha"),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{detect, get_sha};
    use crate::db::Database;
    use crate::prelude::*;
    use crate::server::github::{Commit, CommitParent, GitHub, Label};
    use std::cell::RefCell;
    use std::collections::HashMap;

    static COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    static COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    static COMMIT_C: &str = "cccccccccccccccccccccccccccccccccccccccc";

    #[test]
    fn test_detect_and_fetch() {
        let db = Database::temp().unwrap();
        let gh = DummyGitHub::default();

        detect(&db, &gh, "rust-lang/rust", 1, "Test message").unwrap();
        assert!(get_sha(&db, "rust-lang/rust", 1).unwrap().is_none());

        gh.set_commit(
            "rust-lang/rust",
            COMMIT_A,
            Commit {
                sha: COMMIT_A.into(),
                parents: vec![
                    CommitParent {
                        sha: COMMIT_B.into(),
                    },
                    CommitParent {
                        sha: COMMIT_C.into(),
                    },
                ],
            },
        );
        detect(
            &db,
            &gh,
            "rust-lang/rust",
            1,
            &format!(
                r#"
                    Try build passed.
                    <!-- homu: {{"type": "TryBuildCompleted", "merge_sha": "{}"}} -->
                "#,
                COMMIT_A
            ),
        )
        .unwrap();
        let commit = get_sha(&db, "rust-lang/rust", 1).unwrap().unwrap();
        assert_eq!(commit.merge_sha.as_str(), COMMIT_A);
        assert_eq!(commit.base_sha.as_str(), COMMIT_B);
    }

    #[derive(Default)]
    struct DummyGitHub {
        commits: RefCell<HashMap<(String, String), Commit>>,
    }

    impl DummyGitHub {
        fn set_commit(&self, repo: &str, sha: &str, commit: Commit) {
            self.commits
                .borrow_mut()
                .insert((repo.to_string(), sha.to_string()), commit);
        }
    }

    impl GitHub for DummyGitHub {
        fn username(&self) -> Fallible<String> {
            unimplemented!();
        }

        fn post_comment(&self, _issue_url: &str, _body: &str) -> Fallible<()> {
            unimplemented!();
        }

        fn list_labels(&self, _issue_url: &str) -> Fallible<Vec<Label>> {
            unimplemented!();
        }

        fn add_label(&self, _issue_url: &str, _label: &str) -> Fallible<()> {
            unimplemented!();
        }

        fn remove_label(&self, _issue_url: &str, _label: &str) -> Fallible<()> {
            unimplemented!();
        }

        fn list_teams(&self, _org: &str) -> Fallible<HashMap<String, usize>> {
            unimplemented!();
        }

        fn team_members(&self, _team: usize) -> Fallible<Vec<String>> {
            unimplemented!();
        }

        fn get_commit(&self, repo: &str, sha: &str) -> Fallible<Commit> {
            Ok(self
                .commits
                .borrow_mut()
                .remove(&(repo.into(), sha.into()))
                .unwrap())
        }
    }
}
