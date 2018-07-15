use errors::*;
use server::Data;

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
    new_label: Option<Label>,
}

impl Message {
    pub fn new() -> Message {
        Message {
            lines: Vec::new(),
            notes: Vec::new(),
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

    pub fn set_label(mut self, label: Label) -> Self {
        self.new_label = Some(label);
        self
    }

    pub fn send(mut self, issue_url: &str, data: &Data) -> Result<()> {
        // Always add a note at the bottom explaining what this is
        self = self.note(
            "information_source",
            "**Crater** is a tool to run experiments across parts of the Rust ecosystem. \
             [Learn more](https://github.com/rust-lang-nursery/crater)",
        );

        let mut message = String::new();
        for line in self.lines {
            message.push_str(&format!(":{}: {}\n", line.emoji, line.content));
        }
        for line in self.notes {
            message.push_str(&format!("\n:{}: {}", line.emoji, line.content));
        }

        data.github.post_comment(issue_url, &message)?;

        if let Some(label) = self.new_label {
            let label = match label {
                Label::ExperimentQueued => &data.config.server.labels.experiment_queued,
                Label::ExperimentCompleted => &data.config.server.labels.experiment_completed,
            };

            // Remove all the labels matching the provided regex
            // If the label is already present don't reapply it though
            let regex = &data.config.server.labels.remove;
            let current_labels = data.github.list_labels(issue_url)?;
            let mut label_already_present = false;
            for current_label in &current_labels {
                if current_label.name == *label {
                    label_already_present = true;
                } else if regex.is_match(&current_label.name) {
                    data.github.remove_label(issue_url, &current_label.name)?;
                }
            }

            if !label_already_present {
                data.github.add_label(issue_url, label)?;
            }
        }

        Ok(())
    }
}
