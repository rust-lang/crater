//! Build timing capture from cargo's `--timings` JSON output.
//!
//! When enabled, the runner adds `--timings` to cargo invocations on nightly
//! toolchains and routes `"reason":"timing-info"` JSON messages through a
//! [`TimingVisitor`]. The [`CollectingTimingVisitor`] accumulates entries in
//! memory; the [`NoopTimingVisitor`] does nothing at close to zero cost.

use std::mem;

use crate::config::Config;
use crate::prelude::*;
use crate::toolchain::Toolchain;

/// A single timing record emitted by cargo for one compilation unit.
///
/// Cargo produces these as JSON lines with `"reason":"timing-info"` when
/// invoked with `--timings --message-format=json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingInfo {
    pub package_id: String,
    pub target: TimingTarget,
    pub mode: String,
    pub duration: f64,
    pub rmeta_time: Option<f64>,
}

/// The target (lib, bin, etc.) that was compiled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingTarget {
    pub kind: Vec<String>,
    pub crate_types: Vec<String>,
    pub name: String,
    pub src_path: String,
    pub edition: String,
    pub doc: bool,
    pub doctest: bool,
    pub test: bool,
}

/// Visitor trait for processing timing data emitted by cargo.
pub trait TimingVisitor {
    /// Called for each `timing-info` message parsed from cargo output.
    fn visit_timing(&mut self, timing: TimingInfo);

    /// Drains and returns all collected timing entries.
    fn take_results(&mut self) -> Vec<TimingInfo>;

    /// Returns `true` when this visitor is actively capturing (controls
    /// whether `--timings` is appended to cargo args).
    fn is_capturing(&self) -> bool;
}

/// No-op visitor used when capture is disabled or in agent mode.
/// Each method is an empty body — close to zero cost (only the dynamic dispatch overhead).
pub struct NoopTimingVisitor;

impl TimingVisitor for NoopTimingVisitor {
    fn visit_timing(&mut self, _timing: TimingInfo) {}

    fn take_results(&mut self) -> Vec<TimingInfo> {
        Vec::new()
    }

    fn is_capturing(&self) -> bool {
        false
    }
}

/// Collects timing entries into a `Vec<TimingInfo>`.
#[derive(Default)]
pub struct CollectingTimingVisitor {
    timings: Vec<TimingInfo>,
}

impl CollectingTimingVisitor {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TimingVisitor for CollectingTimingVisitor {
    fn visit_timing(&mut self, timing: TimingInfo) {
        self.timings.push(timing);
    }

    fn take_results(&mut self) -> Vec<TimingInfo> {
        mem::take(&mut self.timings)
    }

    fn is_capturing(&self) -> bool {
        true
    }
}

/// Decides whether to capture timings for the given context.
pub fn should_capture_timings(config: &Config, toolchain: &Toolchain, is_agent_mode: bool) -> bool {
    config.capture_timings // Only if it's enabled
    && toolchain.is_nightly() // Required for --timings
    && !is_agent_mode // TODO: remove this when implemented for agent mode
}

/// Creates the appropriate visitor based on config, toolchain, and mode.
pub fn create_visitor(
    config: &Config,
    toolchain: &Toolchain,
    is_agent_mode: bool,
) -> Box<dyn TimingVisitor> {
    if should_capture_timings(config, toolchain, is_agent_mode) {
        Box::new(CollectingTimingVisitor::new())
    } else {
        Box::new(NoopTimingVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_timing_info() {
        let json = r#"{
            "reason": "timing-info",
            "package_id": "serde 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)",
            "target": {
                "kind": ["lib"],
                "crate_types": ["lib"],
                "name": "serde",
                "src_path": "/path/to/src/lib.rs",
                "edition": "2021",
                "doc": true,
                "doctest": true,
                "test": true
            },
            "mode": "build",
            "duration": 12.5,
            "rmeta_time": 8.3
        }"#;

        let timing: TimingInfo = serde_json::from_str(json).unwrap();
        assert_eq!(
            timing.package_id,
            "serde 1.0.0 (registry+https://github.com/rust-lang/crates.io-index)"
        );
        assert_eq!(timing.target.name, "serde");
        assert_eq!(timing.target.kind, vec!["lib"]);
        assert_eq!(timing.mode, "build");
        assert!((timing.duration - 12.5).abs() < f64::EPSILON);
        assert!((timing.rmeta_time.unwrap() - 8.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_deserialize_timing_info_no_rmeta() {
        let json = r#"{
            "package_id": "foo 0.1.0",
            "target": {
                "kind": ["bin"],
                "crate_types": ["bin"],
                "name": "foo",
                "src_path": "/path/to/main.rs",
                "edition": "2021",
                "doc": false,
                "doctest": false,
                "test": false
            },
            "mode": "build",
            "duration": 1.0,
            "rmeta_time": null
        }"#;

        let timing: TimingInfo = serde_json::from_str(json).unwrap();
        assert!(timing.rmeta_time.is_none());
    }

    #[test]
    fn test_should_capture_timings() {
        use std::str::FromStr;

        let mut config = Config::default();

        let nightly = Toolchain::from_str("nightly-2024-01-01").unwrap();
        let stable = Toolchain::from_str("stable").unwrap();
        let ci = Toolchain::from_str("try#0000000000000000000000000000000000000000").unwrap();

        // Disabled by default
        assert!(!should_capture_timings(&config, &nightly, false));

        // Enabled config + nightly + local = true
        config.capture_timings = true;
        assert!(should_capture_timings(&config, &nightly, false));

        // Enabled config + stable + local = false (not nightly)
        assert!(!should_capture_timings(&config, &stable, false));

        // Enabled config + nightly + agent = false (agent mode)
        assert!(!should_capture_timings(&config, &nightly, true));

        // Enabled config + CI build + local = true (CI uses nightly)
        assert!(should_capture_timings(&config, &ci, false));
    }
}
