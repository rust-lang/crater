use crater::experiments::ExperimentDBRecord;
use crater::report::ReportWriter;
use crater::results::EncodingType;
use crater::{config::Config, db::QueryUtils};
use failure::Fallible;
use mime::{self, Mime};
use std::{borrow::Cow, fmt, path::Path};

fn main() {
    let mut env = env_logger::Builder::new();
    env.filter_module("test_report", log::LevelFilter::Info);
    env.filter_module("crater", log::LevelFilter::Info);
    env.filter_module("rustwide", log::LevelFilter::Info);
    if let Ok(content) = std::env::var("RUST_LOG") {
        env.parse_filters(&content);
    }
    rustwide::logging::init_with(env.build());
    let config: Config = toml::from_str(&std::fs::read_to_string("config.toml").unwrap()).unwrap();
    let db = crater::db::Database::open_at(std::path::Path::new("crater.db")).unwrap();
    let experiments = db
        .query("SELECT * FROM experiments;", [], |r| {
            ExperimentDBRecord::from_row(r)
        })
        .unwrap();
    let experiments: Vec<_> = experiments
        .into_iter()
        .map(|record| record.into_experiment())
        .collect::<Fallible<_>>()
        .unwrap();
    let ex = experiments.iter().find(|e| e.name == "pr-118920").unwrap();
    let rdb = crater::results::DatabaseDB::new(&db);

    log::info!("Getting crates...");

    let crates = ex.get_crates(&db).unwrap();
    let writer = NullWriter;

    log::info!("Starting report generation...");
    log::info!(
        "@ {:?}",
        nix::sys::resource::getrusage(nix::sys::resource::UsageWho::RUSAGE_SELF)
            .unwrap()
            .max_rss()
    );
    crater::report::gen(&rdb, ex, &crates, &writer, &config, false).unwrap();
    log::info!(
        "@ {:?}",
        nix::sys::resource::getrusage(nix::sys::resource::UsageWho::RUSAGE_SELF)
            .unwrap()
            .max_rss()
    );
}

#[derive(Debug)]
struct NullWriter;

impl ReportWriter for NullWriter {
    fn write_bytes<P: AsRef<Path>>(
        &self,
        _path: P,
        _b: &[u8],
        _mime: &Mime,
        _encoding_type: EncodingType,
    ) -> Fallible<()> {
        // no-op
        Ok(())
    }
    fn write_string<P: AsRef<Path>>(&self, _path: P, _s: Cow<str>, _mime: &Mime) -> Fallible<()> {
        // no-op
        Ok(())
    }
}

impl fmt::Display for NullWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
