use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::Experiment;
use crate::prelude::*;
use crate::report::{compare, ReportWriter};
use crate::results::{EncodedLog, EncodingType, ReadResults};
use flate2::{write::GzEncoder, Compression};
use std::collections::HashMap;
use tar::{Builder as TarBuilder, Header as TarHeader};

#[derive(Serialize)]
pub struct Archive {
    name: String,
    path: String,
}

pub fn write_logs_archives<DB: ReadResults, W: ReportWriter>(
    db: &DB,
    ex: &Experiment,
    crates: &[Crate],
    dest: &W,
    config: &Config,
) -> Fallible<Vec<Archive>> {
    let mut archives = Vec::new();
    let mut all = TarBuilder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut by_comparison = HashMap::new();

    for krate in crates {
        if config.should_skip(krate) {
            continue;
        }

        let res1 = db.load_test_result(ex, &ex.toolchains[0], krate)?;
        let res2 = db.load_test_result(ex, &ex.toolchains[1], krate)?;
        let comparison = compare(config, krate, res1, res2);

        for tc in &ex.toolchains {
            let log = db
                .load_log(ex, tc, krate)
                .and_then(|c| c.ok_or_else(|| err_msg("missing logs")))
                .with_context(|_| format!("failed to read log of {} on {}", krate, tc));

            let log_bytes: EncodedLog = match log {
                Ok(l) => l,
                Err(e) => {
                    crate::utils::report_failure(&e);
                    continue;
                }
            };

            let log_bytes = log_bytes.to_plain()?;
            let log_bytes = log_bytes.as_slice();

            let path = format!("{}/{}/{}.txt", comparison, krate.id(), tc);

            let mut header = TarHeader::new_gnu();
            header.set_size(log_bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();

            all.append_data(&mut header, &path, log_bytes)?;
            by_comparison
                .entry(comparison)
                .or_insert_with(|| {
                    TarBuilder::new(GzEncoder::new(Vec::new(), Compression::default()))
                })
                .append_data(&mut header, &path, log_bytes)?;
        }
    }

    let data = all.into_inner()?.finish()?;
    dest.write_bytes(
        "logs-archives/all.tar.gz",
        data,
        &"application/gzip".parse().unwrap(),
        EncodingType::Plain,
    )?;

    archives.push(Archive {
        name: "All the crates".to_string(),
        path: "logs-archives/all.tar.gz".to_string(),
    });

    for (comparison, archive) in by_comparison.drain() {
        let data = archive.into_inner()?.finish()?;
        dest.write_bytes(
            &format!("logs-archives/{}.tar.gz", comparison),
            data,
            &"application/gzip".parse().unwrap(),
            EncodingType::Plain,
        )?;

        archives.push(Archive {
            name: format!("{} crates", comparison),
            path: format!("logs-archives/{}.tar.gz", comparison),
        });
    }

    Ok(archives)
}

#[cfg(test)]
mod tests {
    use super::write_logs_archives;
    use crate::actions::{Action, ActionsCtx, CreateExperiment};
    use crate::config::Config;
    use crate::db::Database;
    use crate::experiments::Experiment;
    use crate::prelude::*;
    use crate::report::DummyWriter;
    use crate::results::{DatabaseDB, EncodingType, FailureReason, TestResult, WriteResults};
    use flate2::read::GzDecoder;
    use mime::Mime;
    use std::io::Read;
    use tar::Archive;

    #[test]
    fn test_logs_archives_generation() {
        crate::logs::init_test();

        let config = Config::default();
        let db = Database::temp().unwrap();
        let writer = DummyWriter::default();
        let ctx = ActionsCtx::new(&db, &config);

        crate::crates::lists::setup_test_lists(&db, &config).unwrap();

        // Create a dummy experiment
        CreateExperiment::dummy("dummy").apply(&ctx).unwrap();
        let ex = Experiment::get(&db, "dummy").unwrap().unwrap();
        let crate1 = &ex.get_crates(&db).unwrap()[0];
        let crate2 = &ex.get_crates(&db).unwrap()[1];

        // Fill some dummy results into the database
        let results = DatabaseDB::new(&db);
        results
            .record_result(
                &ex,
                &ex.toolchains[0],
                &crate1,
                None,
                &config,
                EncodingType::Gzip,
                || {
                    info!("tc1 crate1");
                    Ok(TestResult::TestPass)
                },
            )
            .unwrap();
        results
            .record_result(
                &ex,
                &ex.toolchains[1],
                &crate1,
                None,
                &config,
                EncodingType::Plain,
                || {
                    info!("tc2 crate1");
                    Ok(TestResult::BuildFail(FailureReason::Unknown))
                },
            )
            .unwrap();
        results
            .record_result(
                &ex,
                &ex.toolchains[0],
                &crate2,
                None,
                &config,
                EncodingType::Gzip,
                || {
                    info!("tc1 crate2");
                    Ok(TestResult::TestPass)
                },
            )
            .unwrap();
        results
            .record_result(
                &ex,
                &ex.toolchains[1],
                &crate2,
                None,
                &config,
                EncodingType::Plain,
                || {
                    info!("tc2 crate2");
                    Ok(TestResult::TestPass)
                },
            )
            .unwrap();

        // Generate all the archives
        let archives = write_logs_archives(
            &results,
            &ex,
            &ex.get_crates(&db).unwrap(),
            &writer,
            &config,
        )
        .unwrap();

        // Ensure the correct list of archives is returned
        let mut archives_paths = archives.into_iter().map(|a| a.path).collect::<Vec<_>>();
        archives_paths.sort();
        assert_eq!(
            &archives_paths,
            &[
                "logs-archives/all.tar.gz",
                "logs-archives/regressed.tar.gz",
                "logs-archives/test-pass.tar.gz",
            ]
        );

        // Load the content of all the archives
        let mime: Mime = "application/gzip".parse().unwrap();
        let all_content = writer.get("logs-archives/all.tar.gz", &mime);
        let mut all = Archive::new(GzDecoder::new(all_content.as_slice()));
        let regressed_content = writer.get("logs-archives/regressed.tar.gz", &mime);
        let mut regressed = Archive::new(GzDecoder::new(regressed_content.as_slice()));
        let test_pass_content = writer.get("logs-archives/test-pass.tar.gz", &mime);
        let mut test_pass = Archive::new(GzDecoder::new(test_pass_content.as_slice()));

        macro_rules! check_content {
            ($archive:ident: { $($file:expr => $match:expr,)* }) => {{
                let mut count = 0;
                for entry in $archive.entries().unwrap() {
                    count += 1;
                    let mut entry = entry.unwrap();

                    // Ensure the contained files are readable
                    assert_eq!(entry.header().mode().unwrap(), 0o644);

                    let mut content = String::new();
                    entry.read_to_string(&mut content).unwrap();

                    let path = entry.path().unwrap();
                    let path = path.to_string_lossy().to_owned();
                    $(
                        if &path == &$file {
                            assert!(content.contains($match));
                            continue;
                        }
                    )*

                    panic!("unknown path in archive: {}", path);
                }

                let mut total = 0;
                $(let _ = $match; total += 1;)*
                assert_eq!(count, total);
            }}
        }

        // Check all.tar.gz
        check_content!(all: {
            format!("regressed/{}/{}.txt", crate1.id(), ex.toolchains[0]) => "tc1 crate1",
            format!("regressed/{}/{}.txt", crate1.id(), ex.toolchains[1]) => "tc2 crate1",
            format!("test-pass/{}/{}.txt", crate2.id(), ex.toolchains[0]) => "tc1 crate2",
            format!("test-pass/{}/{}.txt", crate2.id(), ex.toolchains[1]) => "tc2 crate2",
        });

        // Check regressed.tar.gz
        check_content!(regressed: {
            format!("regressed/{}/{}.txt", crate1.id(), ex.toolchains[0]) => "tc1 crate1",
            format!("regressed/{}/{}.txt", crate1.id(), ex.toolchains[1]) => "tc2 crate1",
        });

        // Check test-pass.tar.gz
        check_content!(test_pass: {
            format!("test-pass/{}/{}.txt", crate2.id(), ex.toolchains[0]) => "tc1 crate2",
            format!("test-pass/{}/{}.txt", crate2.id(), ex.toolchains[1]) => "tc2 crate2",
        });
    }
}
