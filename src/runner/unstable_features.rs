use crate::prelude::*;
use crate::results::TestResult;
use crate::runner::tasks::TaskCtx;
use cargo_metadata::Package;
use rustwide::Build;
use std::collections::HashSet;
use std::path::Path;
use walkdir::{DirEntry, WalkDir};

pub(super) fn find_unstable_features(
    _ctx: &TaskCtx,
    build: &Build,
    _local_packages_id: &[Package],
) -> Fallible<TestResult> {
    let mut features = HashSet::new();

    for entry in WalkDir::new(build.host_source_dir())
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry?;
        if !entry
            .file_name()
            .to_str()
            .map(|s| s.contains(".rs"))
            .unwrap_or(false)
        {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }

        let new_features = parse_features(entry.path())?;

        for feature in new_features {
            features.insert(feature);
        }
    }

    let mut features: Vec<_> = features.into_iter().collect();
    features.sort();
    for feature in features {
        info!("unstable-feature: {}", feature);
    }

    Ok(TestResult::TestPass)
}

fn parse_features(path: &Path) -> Fallible<Vec<String>> {
    let mut features = Vec::new();
    let contents = ::std::fs::read_to_string(path)?;
    for (hash_idx, _) in contents.match_indices('#') {
        let contents = &contents[hash_idx + 1..];
        let contents = eat_token(Some(contents), "!").or(Some(contents));
        let contents = eat_token(contents, "[");
        let contents = eat_token(contents, "feature");
        let new_features = parse_list(contents, "(", ")");
        features.extend_from_slice(&new_features);
    }

    Ok(features)
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.'))
        .unwrap_or(false)
}

fn eat_token<'a>(s: Option<&'a str>, tok: &str) -> Option<&'a str> {
    eat_whitespace(s).and_then(|s| s.strip_prefix(tok))
}

fn eat_whitespace(s: Option<&str>) -> Option<&str> {
    s.and_then(|s| {
        #[allow(clippy::manual_map)]
        if let Some(i) = s.find(|c: char| !c.is_whitespace()) {
            Some(&s[i..])
        } else {
            None
        }
    })
}

fn parse_list(s: Option<&str>, open: &str, close: &str) -> Vec<String> {
    let s = eat_whitespace(s);
    let s = eat_token(s, open);
    if let Some(s) = s {
        if let Some(i) = s.find(close) {
            let s = &s[..i];
            return s.split(',').map(|s| s.trim().to_string()).collect();
        }
    }

    Vec::new()
}
