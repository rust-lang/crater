use crate::prelude::*;
use crate::server::github::GitHub;
use crate::server::{Data, GithubData};
use std::collections::BTreeMap;
use std::fmt::Write;

pub enum Label {
    ExperimentQueued,
    ExperimentCompleted,
}

struct Line {
    emoji: String,
    content: String,
}

pub struct Message {
    lines: Vec<Line>,
    notes: Vec<Line>,
    footnotes: BTreeMap<String, String>,
    new_label: Option<Label>,
}

impl Message {
    pub fn new() -> Message {
        Message {
            lines: Vec::new(),
            notes: Vec::new(),
            footnotes: BTreeMap::new(),
            new_label: None,
        }
    }

    pub fn line<S1: Into<String>, S2: Into<String>>(mut self, emoji: S1, content: S2) -> Self {
        self.lines.push(Line {
            emoji: emoji.into(),
            content: content.into(),
        });
        self
    }

    pub fn note<S1: Into<String>, S2: Into<String>>(mut self, emoji: S1, content: S2) -> Self {
        self.notes.push(Line {
            emoji: emoji.into(),
            content: content.into(),
        });
        self
    }

    pub fn footnote<S1: Into<String>, S2: Into<String>>(mut self, key: S1, content: S2) -> Self {
        self.footnotes.insert(key.into(), content.into());
        self
    }

    pub fn set_label(mut self, label: Label) -> Self {
        self.new_label = Some(label);
        self
    }

    pub fn send(mut self, issue_url: &str, data: &Data, github_data: &GithubData) -> Fallible<()> {
        // Always add a note at the bottom explaining what this is
        self = self.note(
            "information_source",
            format!(
                "**Crater** is a tool to run experiments across parts of the Rust ecosystem. \
                 [Learn more]({})",
                crate::CRATER_REPO_URL,
            ),
        );

        let mut message = String::new();
        for line in self.lines {
            writeln!(&mut message, ":{}: {}", line.emoji, line.content).unwrap();
        }
        for line in self.notes {
            write!(&mut message, "\n:{}: {}", line.emoji, line.content).unwrap();
        }
        for (key, content) in self.footnotes {
            write!(&mut message, "\n[^{key}]: {content}").unwrap();
        }

        github_data.api.post_comment(issue_url, &message)?;

        if let Some(label) = self.new_label {
            let label = match label {
                Label::ExperimentQueued => &data.config.server.labels.experiment_queued,
                Label::ExperimentCompleted => &data.config.server.labels.experiment_completed,
            };

            // Remove all the labels matching the provided regex
            // If the label is already present don't reapply it though
            let regex = &data.config.server.labels.remove;
            let current_labels = github_data.api.list_labels(issue_url)?;
            let mut label_already_present = false;
            for current_label in &current_labels {
                if current_label.name == *label {
                    label_already_present = true;
                } else if regex.is_match(&current_label.name) {
                    github_data
                        .api
                        .remove_label(issue_url, &current_label.name)?;
                }
            }

            if !label_already_present {
                github_data.api.add_label(issue_url, label)?;
            }
        }

        Ok(())
    }
}
