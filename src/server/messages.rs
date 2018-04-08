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
            "**Crater** is a tool to run experiments across the whole Rust ecosystem. \
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

        Ok(())
    }
}
