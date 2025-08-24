//! We use crates.io database dumps to find the list of crates/versions.
//!
//! This is somewhat unorthodox, but the git index parsing (at least via the crates-index crate)
//! consumes a lot of memory (gigabytes). The implementation here is able to use much less (idling
//! at ~150MB or so after it's done) and produces roughly equivalent results.

use crate::crates::{lists::List, Crate};
use crate::prelude::*;
use rawzip::{CompressionMethod, ZipLocator};
use reqwest::header::HeaderValue;
use smol_str::SmolStr;
use std::collections::{HashMap, HashSet};
use std::io::Read;

pub(crate) struct RegistryList;

struct DbDumpReader {
    version: HeaderValue,
}

impl rawzip::ReaderAt for DbDumpReader {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
        // Treat zero-size reads as no-op.
        //
        // There's not much value in us making a network request - at best it might prove offset is
        // inbounds of the object, but that doesn't really merit the request at this time.
        if buf.is_empty() {
            return Ok(0);
        }

        let mut res = crate::utils::http::prepare_sync(reqwest::Method::GET, BASE_URL)
            .header(
                reqwest::header::RANGE,
                format!(
                    "bytes={offset}-{}",
                    1 + offset + u64::try_from(buf.len()).unwrap()
                ),
            )
            .send()
            .map_err(|e| std::io::Error::other(e))?;

        if let Some(version) = res.headers().get("x-amz-version-id") {
            if version != self.version {
                return Err(std::io::Error::other(format!(
                    "wrong version returned in ranged GET, found {:?} but expected {:?}",
                    version, self.version
                )));
            }
        } else {
            return Err(std::io::Error::other(format!(
                "missing version ID in range get ({:?})",
                res,
            )));
        }

        let l = buf.len();
        let mut read = 0;
        while read < buf.len() {
            match res.read(&mut buf[read..]) {
                Ok(l) => read += l,
                Err(e) => {
                    if read == 0 {
                        return Err(e);
                    } else {
                        return Ok(read);
                    }
                }
            }
        }
        info!(
            "requesting {}..{}: read {:?} (out of {})",
            offset,
            offset + buf.len() as u64,
            read,
            l
        );
        Ok(read)
    }
}

#[derive(Debug, Deserialize)]
struct CrateRecord {
    name: SmolStr,
    id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct VersionRecord {
    crate_id: u64,
    id: u64,
    #[serde(deserialize_with = "crates_io_bool")]
    yanked: bool,
    created_at: jiff::Timestamp,
    // the version number
    num: SmolStr,
}

#[derive(Debug, Deserialize)]
struct DependencyRecord {
    // this is the crate we're depending on. the version we're depending on is described by `req`
    // (not listed here) as a semver selector.
    crate_id: u64,
    // this identifies the version that is depending on the crate
    version_id: u64,
}

const BASE_URL: &str = "https://crates-io.s3.us-west-1.amazonaws.com/db-dump.zip";

impl List for RegistryList {
    const NAME: &'static str = "registry";

    fn fetch(&self) -> Fallible<Vec<Crate>> {
        let head_response =
            crate::utils::http::prepare_sync(reqwest::Method::HEAD, BASE_URL).send()?;

        let Some(len) = head_response.headers().get(reqwest::header::CONTENT_LENGTH) else {
            anyhow::bail!("missing content-length in response: {head_response:?}");
        };
        let len = len.to_str()?.parse::<u64>()?;

        // We pin the version we're reading from so we don't end up reading from two different
        // files while streaming (partial) contents. Stale versions are retained for at least 24 hours today (per
        // S3 lifecycle configuration) so this should work fine.
        //
        // Note: this actually has no effect today, the version ID is ignored by cloudfront's cache
        // (we don't include query strings in the cache). Maybe we could hit S3 directly instead...
        let Some(version) = head_response.headers().get("x-amz-version-id") else {
            anyhow::bail!("missing x-amz-version-id in response: {head_response:?}");
        };

        info!("Downloading crates.io db dump (version ID: {:?})", version);

        let locator = ZipLocator::new();
        let mut buffer = vec![0u8; rawzip::RECOMMENDED_BUFFER_SIZE];

        let archive = locator
            .max_search_space(4096)
            .locate_in_reader(
                DbDumpReader {
                    version: version.clone(),
                },
                &mut buffer,
                len,
            )
            .map_err(|e| e.1)?;

        // Collect all crates.
        //
        // Crate ID -> (crate name, preferred version, # of reverse dependencies)
        let mut crates: HashMap<u64, (SmolStr, Option<VersionRecord>, u64)> = HashMap::new();
        let mut entries = archive.entries(&mut buffer);

        let mut crates_entry = None;
        let mut dependencies_entry = None;
        let mut versions_entry = None;
        while let Some(entry) = entries.next_entry()? {
            match entry.file_path().try_normalize()?.as_str() {
                "data/crates.csv" => crates_entry = Some(entry.wayfinder()),
                "data/dependencies.csv" => dependencies_entry = Some(entry.wayfinder()),
                "data/versions.csv" => versions_entry = Some(entry.wayfinder()),
                _ => continue,
            }

            assert_eq!(entry.compression_method(), CompressionMethod::Deflate);
        }

        let entry = archive.get_entry(crates_entry.unwrap())?;
        let mut rdr = csv::Reader::from_reader(entry.verifying_reader(
            flate2::read::DeflateDecoder::new_with_buf(entry.reader(), vec![0; 64 * 1024 * 1024]),
        ));
        for record in rdr.deserialize::<CrateRecord>() {
            let record = record?;
            assert!(crates.insert(record.id, (record.name, None, 0)).is_none());
        }

        info!("read {} crates", crates.len());

        // Now find the last crate version to be published, and store it.
        let entry = archive.get_entry(versions_entry.unwrap())?;

        let mut rdr = csv::Reader::from_reader(entry.verifying_reader(
            flate2::read::DeflateDecoder::new_with_buf(entry.reader(), vec![0; 64 * 1024 * 1024]),
        ));
        for record in rdr.deserialize::<VersionRecord>() {
            let record = record?;

            if record.yanked {
                continue;
            }

            let krate = crates
                .get_mut(&record.crate_id)
                .expect("all crates in crates.csv");

            // Getting the last published version is what the crates.io github index code did,
            // but we may want to switch this to latest semver version or something else.
            //
            // Maybe use `default_versions` table instead of this?
            if let Some(prev) = krate.1.take() {
                krate.1 = if prev.created_at < record.created_at {
                    Some(record.clone())
                } else {
                    Some(prev)
                };
            } else {
                krate.1 = Some(record.clone());
            }
        }

        let mut versions_selected = HashSet::new();
        for krate in crates.values() {
            if let Some(record) = krate.1.as_ref() {
                versions_selected.insert(record.id);
            }
        }

        info!("selected {} crate versions", versions_selected.len());

        // Now we scan dependencies to compute counts. These are assigned to a crate in general,
        // not by version being depended on (i.e. assume each dependency type is `*`).
        let entry = archive.get_entry(dependencies_entry.unwrap())?;
        let mut rdr = csv::Reader::from_reader(entry.verifying_reader(
            flate2::read::DeflateDecoder::new_with_buf(entry.reader(), vec![0; 64 * 1024 * 1024]),
        ));
        for record in rdr.deserialize::<DependencyRecord>() {
            let record = record?;

            // Ignore unless this is for a version we're interested in.
            if !versions_selected.contains(&record.version_id) {
                continue;
            }

            let krate = crates
                .get_mut(&record.crate_id)
                .expect("all crates in crates.csv");

            krate.2 += 1;
        }
        info!("computed reverse dependency counts");

        let by_name = crates
            .into_iter()
            .filter(|v| v.1 .1.is_some())
            .map(|(_, v)| (v.0, (v.1.unwrap(), v.2)))
            .collect::<HashMap<_, _>>();

        assert_eq!(by_name.len(), versions_selected.len());
        info!("{} unique crate names", by_name.len());

        let mut list = by_name
            .iter()
            .map(|(name, v)| {
                Crate::Registry(RegistryCrate {
                    name: name.clone(),
                    version: v.0.num.clone(),
                })
            })
            .collect::<Vec<_>>();
        list.sort_unstable_by_key(|a| {
            if let Crate::Registry(ref a) = a {
                by_name[&a.name].1
            } else {
                panic!("non-registry crate produced in the registry list");
            }
        });

        Ok(list)
    }
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub struct RegistryCrate {
    pub name: SmolStr,
    pub version: SmolStr,
}

fn crates_io_bool<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    struct Visitor;

    impl serde::de::Visitor<'_> for Visitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("`t` or `f` representing true/false")
        }

        fn visit_str<E>(self, v: &str) -> Result<bool, E>
        where
            E: serde::de::Error,
        {
            match v {
                "t" => Ok(true),
                "f" => Ok(false),
                _ => Err(E::custom(format!("unexpected value: {:?}", v))),
            }
        }
    }

    d.deserialize_str(Visitor)
}
