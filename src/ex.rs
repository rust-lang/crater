use gh_mirrors;
use std::time::Instant;
use RUSTUP_HOME;
use CARGO_HOME;
use std::env;
use std::fs;
use errors::*;
use EXPERIMENT_DIR;
use std::path::{Path, PathBuf};
use crates;
use lists::{self, Crate};
use run;
use std::collections::{HashMap, HashSet};
use serde_json;
use file;
use toolchain::{self, Toolchain};
use util;
use std::fmt::{self, Formatter, Display};
use log;
use toml_frobber;
use TEST_DIR;

fn ex_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name)
}

fn shafile(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("shas.json")
}

fn config_file(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("config.json")
}

fn froml_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("fromls")
}

fn froml_path(ex_name: &str, name: &str, vers: &str) -> PathBuf {
    froml_dir(ex_name).join(format!("{}-{}.Cargo.toml", name, vers))
}

#[derive(Serialize, Deserialize)]
struct Experiment {
    crates: Vec<Crate>,
    toolchains: Vec<Toolchain>,
}

/// Define an experiment, including selecting the crates to be tested
pub fn define(ex_name: &str, toolchains: &[&str]) -> Result<()> {
    let crates = lists::read_all_lists()?;
    define_(ex_name, toolchains, crates)
}

pub fn define_demo(ex_name: &str, toolchains: &[&str]) -> Result<()> {
    log!("defining demo");
    let demo_crate = "lazy_static";
    let demo_gh_app = "brson/basic-http-server";
    let mut found_demo_crate = false;
    let crates = lists::read_all_lists()?.into_iter().filter(|c| {
        match *c {
            Crate::Version(ref c, _) => {
                if c == demo_crate && !found_demo_crate {
                    found_demo_crate = true;
                    true
                } else {
                    false
                }
            }
            Crate::Repo(ref r) => {
                r.contains(demo_gh_app)
            }
        }
    }).collect();
    define_(ex_name, toolchains, crates)
}

pub fn define_(ex_name: &str, toolchains: &[&str],
               crates: Vec<Crate>) -> Result<()> {
    let mut tcs = Vec::new();
    for tc in toolchains {
        tcs.push(toolchain::parse_toolchain(tc)?);
    }

    log!("defining experiment {} for {} crates", ex_name, crates.len());
    let ex = Experiment {
        crates: crates,
        toolchains: tcs,
    };
    fs::create_dir_all(&ex_dir(ex_name))?;
    let json = serde_json::to_string(&ex)
        .chain_err(|| "unable to serialize experiment config")?;
    log!("writing ex config to {}", config_file(ex_name).display());
    file::write_string(&config_file(ex_name), &json)?;
    Ok(())
}

fn load_config(ex_name: &str) -> Result<Experiment> {
    let config = file::read_string(&config_file(ex_name))?;
    serde_json::from_str(&config)
        .chain_err(|| "unable to deserialize experiment config")
}

pub fn fetch_gh_mirrors(ex_name: &str) -> Result<()> {
    let config = load_config(ex_name)?;
    for c in &config.crates {
        match *c {
            Crate::Repo(ref url) => {
                gh_mirrors::fetch(url)?;
            }
            _ => ()
        }
    }

    Ok(())
}

pub fn capture_shas(ex_name: &str) -> Result<()> {
    let mut shas: HashMap<String, String> = HashMap::new();
    let config = load_config(ex_name)?;
    for krate in config.crates {
        match krate {
            Crate::Repo(url) => {
                let dir = gh_mirrors::repo_dir(&url)?;
                let r = run::run_capture(Some(&dir),
                                         "git",
                                         &["log", "-n1", "--pretty=%H"],
                                         &[]);

                match r {
                    Ok((stdout, stderr)) => {
                        if let Some(shaline) = stdout.get(0) {
                            if shaline.len() > 0 {
                                log!("sha for {}: {}", url, shaline);
                                shas.insert(url, shaline.to_string());
                            } else {
                                log_err!("bogus output from git log for {}", dir.display());
                            }
                        } else {
                            log_err!("bogus output from git log for {}", dir.display());
                        }
                    }
                    Err(e) => {
                        log_err!("unable to capture sha for {}: {}", dir.display(), e);
                    }
                }
            }
            _ => ()
        }
    }

    fs::create_dir_all(&ex_dir(ex_name))?;
    let shajson = serde_json::to_string(&shas)
        .chain_err(|| "unable to serialize json")?;
    log!("writing shas to {}", shafile(ex_name).display());
    file::write_string(&shafile(ex_name), &shajson)?;

    Ok(())
}

fn load_shas(ex_name: &str) -> Result<HashMap<String, String>> {
    let shas = file::read_string(&shafile(ex_name))?;
    let shas = serde_json::from_str(&shas)
        .chain_err(|| "unable to deserialize json")?;
    Ok(shas)
}

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Clone)]
pub enum ExCrate {
    Version(String, String), // name, vers
    Repo(String, String) // url, sha
}

impl Display for ExCrate {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        let s = match *self {
            ExCrate::Version(ref n, ref v) => format!("{}-{}", n, v),
            ExCrate::Repo(ref u, ref s) => format!("{}#{}", u, s)
        };
        s.fmt(f)
    }
}

fn crate_to_ex_crate(c: Crate, shas: &HashMap<String, String>) -> Result<ExCrate> {
    match c {
        Crate::Version(n, v) => Ok(ExCrate::Version(n, v)),
        Crate::Repo(u) => {
            if let Some(sha) = shas.get(&u) {
                Ok(ExCrate::Repo(u, sha.to_string()))
            } else {
                Err(format!("missing sha for {}", u).into())
            }
        }
    }
}

fn ex_crate_to_crate(c: ExCrate) -> Result<Crate> {
    match c {
        ExCrate::Version(n, v) => Ok(Crate::Version(n, v)),
        ExCrate::Repo(u, _) => Ok(Crate::Repo(u))
    }
}

fn ex_crates_and_dirs(ex_name: &str) -> Result<Vec<(ExCrate, PathBuf)>> {
    let config = load_config(ex_name)?;
    let shas = load_shas(ex_name)?;
    let crates = config.crates.clone().into_iter().filter_map(|c| {
        let c = crate_to_ex_crate(c, &shas);
        if let Err(e) = c {
            util::report_error(&e);
            return None;
        }
        let c = c.expect("");
        let dir = crates::crate_dir(&c);
        if let Err(e) = dir {
            util::report_error(&e);
            return None;
        }
        let dir = dir.expect("");
        Some((c, dir))
    });
    Ok(crates.collect())
}

pub fn download_crates(ex_name: &str) -> Result<()> {
    crates::prepare(&ex_crates_and_dirs(ex_name)?)
}

pub fn frob_tomls(ex_name: &str) -> Result<()> {
    for (krate, dir) in ex_crates_and_dirs(ex_name)? {
        match krate {
            ExCrate::Version(ref name, ref vers) => {
                let out = froml_path(ex_name, name, vers);
                let r = toml_frobber::frob_toml(&dir, name, vers, &out);
                if let Err(e) = r {
                    log!("couldn't frob: {}", e);
                    util::report_error(&e);
                }
            }
            _ => ()
        }
    }

    Ok(())
}

fn lockfile_dir(ex_name: &str) -> PathBuf {
    Path::new(EXPERIMENT_DIR).join(ex_name).join("lockfiles")
}

fn lockfile(ex_name: &str, crate_: &ExCrate) -> PathBuf {
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version(ref n, ref v) => (n.to_string(), v.to_string()),
        _ => panic!("unimplemented crate type in `lockfile`"),
    };
    lockfile_dir(ex_name).join(format!("{}-{}.lock", crate_name, crate_vers))
}

pub fn capture_lockfiles(ex_name: &str, toolchain: &str, recapture_existing: bool) -> Result<()> {
    fs::create_dir_all(&lockfile_dir(ex_name))?;

    let crates = ex_crates_and_dirs(ex_name)?;

    for (ref c, ref dir) in crates {
        if dir.join("Cargo.lock").exists() {
            log!("crate {} has a lockfile. skipping", c);
            continue;
        }
        let captured_lockfile = lockfile(ex_name, c);
        if captured_lockfile.exists() && !recapture_existing {
            log!("skipping existing lockfile for {}", c);
            continue;
        }
        let r = crates::with_work_crate(c, |path| {
            with_frobbed_toml(ex_name, c, path)?;
            capture_lockfile(ex_name, c, path, toolchain)
        }).chain_err(|| format!("failed to generate lockfile for {}", c));
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())
}

fn capture_lockfile(ex_name: &str, crate_: &ExCrate, path: &Path, toolchain: &str) -> Result<()> {
    let manifest_path = path.join("Cargo.toml").to_string_lossy().to_string();
    let args = &["generate-lockfile",
                 "--manifest-path",
                 &*manifest_path];
    toolchain::run_cargo(toolchain, ex_name, args)
        .chain_err(|| format!("unable to generate lockfile for {}", crate_))?;

    let ref src_lockfile = path.join("Cargo.lock");
    let ref dst_lockfile = lockfile(ex_name, crate_);
    fs::copy(src_lockfile, dst_lockfile)
        .chain_err(|| format!("unable to copy lockfile from {} to {}",
                              src_lockfile.display(), dst_lockfile.display()))?;

    log!("generated lockfile for {} at {}", crate_, dst_lockfile.display());
    
    Ok(())
}

fn with_captured_lockfile(ex_name: &str, crate_: &ExCrate, path: &Path) -> Result<()> {
    let ref dst_lockfile = path.join("Cargo.lock");
    if dst_lockfile.exists() {
        return Ok(());
    }
    let ref src_lockfile = lockfile(ex_name, crate_);
    if src_lockfile.exists() {
        log!("using lockfile {}", src_lockfile.display());
        fs::copy(src_lockfile, dst_lockfile)
            .chain_err(|| format!("unable to copy lockfile from {} to {}",
                                  src_lockfile.display(), dst_lockfile.display()))?;
    }

    Ok(())
}

pub fn fetch_deps(ex_name: &str, toolchain: &str) -> Result<()> {
    let crates = ex_crates_and_dirs(ex_name)?;
    for (ref c, ref dir) in crates {
        let r = crates::with_work_crate(c, |path| {
            with_frobbed_toml(ex_name, c, path)?;
            with_captured_lockfile(ex_name, c, path)?;

            let manifest_path = path.join("Cargo.toml").to_string_lossy().to_string();
            let args = &["fetch",
                         "--locked",
                         "--manifest-path",
                         &*manifest_path];
            toolchain::run_cargo(toolchain, ex_name, args)
                .chain_err(|| format!("unable to fetch deps for {}", c))?;

            Ok(())
        });
        if let Err(e) = r {
            util::report_error(&e);
        }
    }

    Ok(())

}

pub fn prepare_all_toolchains(ex_name: &str) -> Result<()> {
    let config = load_config(ex_name)?;
    for tc in &config.toolchains {
        toolchain::prepare_toolchain_(tc)?;
    }

    Ok(())
}


// Experiment running

fn with_frobbed_toml(ex_name: &str, crate_: &ExCrate, path: &Path) -> Result<()> {
    let (crate_name, crate_vers) = match *crate_ {
        ExCrate::Version(ref n, ref v) => (n.to_string(), v.to_string()),
        _ => return Ok(())
    };
    let ref src_froml = froml_path(ex_name, &crate_name, &crate_vers);
    let ref dst_froml = path.join("Cargo.toml");
    if src_froml.exists() {
        log!("using frobbed toml {}", src_froml.display());
        fs::copy(src_froml, dst_froml)
            .chain_err(|| format!("unable to copy frobbed toml from {} to {}",
                                  src_froml.display(), dst_froml.display()))?;
    }

    Ok(())
}

pub fn run_build_and_test_test(ex_name: &str, toolchain: &str) -> Result<()> {
    run_test(ex_name, toolchain, build_and_test)
}

pub fn run_unstable_features(ex_name: &str, toolchain: &str) -> Result<()> {
    run_test(ex_name, toolchain, find_unstable_features)
}

pub fn run_test<F>(ex_name: &str, toolchain: &str, f: F) -> Result<()>
    where F: Fn(&str, &Path, &str) -> Result<TestResult>
{
    let crates = ex_crates_and_dirs(ex_name)?;

    // Just for reporting progress
    let total_crates = crates.len();
    let mut skipped_crates = 0;
    let mut completed_crates = 0;

    // These should add up to total_crates
    let mut sum_errors = 0;
    let mut sum_fail = 0;
    let mut sum_build_pass = 0;
    let mut sum_test_pass = 0;

    let start_time = Instant::now();

    log!("running {} tests", total_crates);
    for (ref c, ref dir) in crates {
        let r = {
            let existing_result = get_test_result(ex_name, c, toolchain)?;
            if let Some(r) = existing_result {
                skipped_crates += 1;

                log!("skipping crate {}. existing result: {}", c, r);
                log!("delete result file to rerun test: {}",
                     result_file(ex_name, c, toolchain)?.display());
                Ok(r)
            } else {
                completed_crates += 1;

                // SCARY HACK: Crates in the container are built in the mounted
                // ./work/local/test directory. Some of them write files to that directory
                // which end up being owned by root. This command deletes those
                // files by running "rm" in the container. Note especially the "rm .*"
                // command that depends on rm refusing to remove "." and "..".
                let test_dir = Path::new(TEST_DIR);
                if test_dir.exists() {
                    let _ = run_in_docker(ex_name, test_dir,
                                          &["sh", "-c", "rm -rf /test/*; rm -rf /test/.*"]);
                }

                crates::with_work_crate(c, |path| {
                    with_frobbed_toml(ex_name, c, path)?;
                    with_captured_lockfile(ex_name, c, path)?;

                    run_single_test(ex_name, c, path, toolchain, &f)
                })
            }
        };

        match r {
            Err(ref e) => {
                log_err!("error testing crate {}:  {}", c, e);
                util::report_error(e);
            }
            Ok(ref r) => {
                // FIXME: Should errors be recorded?
                record_test_result(ex_name, c, toolchain, *r);
            }
        }

        match r {
            Err(_) => {
                sum_errors += 1;
            }
            Ok(TestResult::Fail) => sum_fail += 1,
            Ok(TestResult::BuildPass) => sum_build_pass +=1,
            Ok(TestResult::TestPass) => sum_test_pass += 1,
        }

        let elapsed = Instant::now().duration_since(start_time).as_secs();
        let seconds_per_test = if completed_crates > 0 {
            (elapsed as f64) / (completed_crates as f64)
        } else {
            0.0
        };
        let remaining_tests = total_crates - completed_crates - skipped_crates;
        let remaining_time = remaining_tests * seconds_per_test as usize;

        let remaining_time_str = if remaining_time < 60 * 8 {
            format!("{:0} seconds", remaining_time)
        } else if remaining_time < 60 * 60 * 8 {
            format!("{:0} minutes", remaining_time / 60)
        } else {
            format!("{:0} hours", remaining_time / 60 / 60)
        };

        log!("progress: {} / {}", completed_crates + skipped_crates, total_crates);
        log!("{} crates tested in {} s. {:.2} s/crate. {} crates remaining. ~{}",
             completed_crates, elapsed, seconds_per_test, remaining_tests, remaining_time_str);
        log!("results: {} fail / {} build-pass / {} test-pass / {} errors",
             sum_fail, sum_build_pass, sum_test_pass, sum_errors);
    }

    Ok(())
}

#[derive(Copy, Clone)]
pub enum TestResult {
    Fail,
    BuildPass,
    TestPass,
}

impl Display for TestResult {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        self.to_string().fmt(f)
    }
}

impl TestResult {
    fn from_str(s: &str) -> Result<TestResult> {
        match s {
            "fail" => Ok(TestResult::Fail),
            "build-pass" => Ok(TestResult::BuildPass),
            "test-pass" => Ok(TestResult::TestPass),
            _ => Err(format!("bogus test result: {}", s).into())
        }
    }
    fn to_string(&self) -> String {
        match *self {
            TestResult::Fail => "fail",
            TestResult::BuildPass => "build-pass",
            TestResult::TestPass => "test-pass",
        }.to_string()
    }
}

fn run_single_test<F>(ex_name: &str, c: &ExCrate, path: &Path,
                      toolchain: &str, f: &F) -> Result<TestResult>
    where F: Fn(&str, &Path, &str) -> Result<TestResult>
{
    let result_dir = result_dir(ex_name, c, toolchain)?;
    if result_dir.exists() {
        util::remove_dir_all(&result_dir)?;
    }
    fs::create_dir_all(&result_dir)?;
    let log_file = result_dir.join("log.txt");

    log::redirect(&log_file, || {
        let tc = toolchain::rustup_toolchain_name(toolchain)?;
        f(ex_name, path, &tc)
    })
}

fn build_and_test(ex_name: &str, path: &Path, rustup_tc: &str) -> Result<TestResult> {
    let tc_arg = &format!("+{}", rustup_tc);
    let build_r = run_in_docker(ex_name, path, &["cargo", tc_arg, "build", "--frozen"]);
    let test_r;

    if build_r.is_ok() {
        test_r = Some(run_in_docker(ex_name, path, &["cargo", tc_arg, "test", "--frozen"]));
    } else {
        test_r = None;
    }

    Ok(match (build_r, test_r) {
        (Err(_), None) => TestResult::Fail,
        (Ok(_), Some(Err(_))) => TestResult::BuildPass,
        (Ok(_), Some(Ok(_))) => TestResult::TestPass,
        (_, _) => unreachable!()
    })
}

fn run_in_docker(ex_name: &str, path: &Path, args: &[&str]) -> Result<()> {

    let test_dir=absolute(path);
    let cargo_home=absolute(Path::new(CARGO_HOME));
    let rustup_home=absolute(Path::new(RUSTUP_HOME));
    let target_dir=absolute(&toolchain::target_dir(ex_name));

    fs::create_dir_all(&test_dir);
    fs::create_dir_all(&cargo_home);
    fs::create_dir_all(&rustup_home);
    fs::create_dir_all(&target_dir);

    let test_mount = &format!("{}:/test", test_dir.display());
    // FIXME this should be read-only https://github.com/rust-lang/cargo/issues/3256
    let cargo_home_mount = &format!("{}:/cargo-home", cargo_home.display());
    let rustup_home_mount = &format!("{}:/rustup-home:ro", rustup_home.display());
    let target_mount = &format!("{}:/target", target_dir.display());

    let image_name = "cargobomb";

    let mut args_ = vec!["run", "-i",
                         "--rm",
                         "-v", test_mount,
                         "-v", cargo_home_mount,
                         "-v", rustup_home_mount,
                         "-v", target_mount,
                         image_name];

    args_.extend_from_slice(args);

    run::run("docker", &args_, &[])
        .chain_err(|| "cargo build failed")?;

    Ok(())
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        let cd = env::current_dir().expect("unable to get current dir");
        cd.join(path)
    }
}

fn result_dir(ex_name: &str, c: &ExCrate, toolchain: &str) -> Result<PathBuf> {
    let tc = toolchain::rustup_toolchain_name(toolchain)?;
    Ok(ex_dir(ex_name).join("res").join(tc).join(crate_to_dir(c)?))
}

fn crate_to_dir(c: &ExCrate) -> Result<String> {
    match *c {
        ExCrate::Version(ref n, ref v) => Ok(format!("reg/{}-{}", n, v)),
        ExCrate::Repo(ref url, ref sha) => {
            let (org, name) = gh_mirrors::gh_url_to_org_and_name(url)?;
            Ok(format!("gh/{}.{}.{}", org, name, sha))
        }
    }
}

fn result_file(ex_name: &str, c: &ExCrate, toolchain: &str) -> Result<PathBuf> {
    Ok(result_dir(ex_name, c, toolchain)?.join("results.txt"))
}

fn record_test_result(ex_name: &str, c: &ExCrate, toolchain: &str, r: TestResult) -> Result<()> {
    let result_dir = result_dir(ex_name, c, toolchain)?;
    fs::create_dir_all(&result_dir)?;
    let result_file = result_file(ex_name, c, toolchain)?;
    log!("test result! ex: {}, c: {}, tc: {}, r: {}", ex_name, c, toolchain, r);
    log!("file: {}", result_file.display());
    file::write_string(&result_file, &r.to_string())?;
    Ok(())
}

fn get_test_result(ex_name: &str, c: &ExCrate, toolchain: &str) -> Result<Option<TestResult>> {
    let result_file = result_file(ex_name, c, toolchain)?;
    if result_file.exists() {
        let s = file::read_string(&result_file)?;
        let r = TestResult::from_str(&s)
            .chain_err(|| format!("invalid test result value: '{}'", s))?;
        Ok(Some(r))
    } else {
        Ok(None)
    }
}

fn find_unstable_features(_ex_name: &str, path: &Path, _rustup_tc: &str) -> Result<TestResult> {
    use walkdir::*;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry.file_name()
            .to_str()
            .map(|s| s.starts_with("."))
            .unwrap_or(false)
    }

    let mut features = HashSet::new();

    for entry in WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry.chain_err(|| "walk dir")?;
        if !entry.file_name().to_str().map(|s| s.contains(".rs")).unwrap_or(false) { continue }
        if !entry.file_type().is_file() { continue }

        let new_features = parse_features(entry.path())?;

        for feature in new_features {
            features.insert(feature);
        }
    }

    let mut features: Vec<_> = features.into_iter().collect();
    features.sort();
    for feature in features {
        log!("unstable-feature: {}", feature);
    }

    Ok(TestResult::TestPass)
}

fn parse_features(path: &Path) -> Result<Vec<String>> {
    let mut features = Vec::new();
    let contents = file::read_string(path)?;
    for (hash_idx, _) in contents.match_indices('#') {
        fn ten_bytes(s: Option<&str>) -> String {
            if let Some(s) = s {
                if s.len() < 10 {
                    s.to_string()
                } else {
                    s[..10].to_string()
                }
            } else {
                String::from("<none>")
            }
        }
        let contents = &contents[hash_idx + 1..];
        let contents = eat_token(Some(contents), "!").or(Some(contents));
        let contents = eat_token(contents, "[");
        let contents = eat_token(contents, "feature");
        let new_features = parse_list(contents, "(", ")");
        features.extend_from_slice(&new_features);
    }

    fn eat_token<'a>(s: Option<&'a str>, tok: &str) -> Option<&'a str> {
        eat_whitespace(s).and_then(|s| {
            if s.starts_with(tok) {
                Some(&s[tok.len()..])
            } else {
                None
            }
        })
    }

    fn eat_whitespace(s: Option<&str>) -> Option<&str> {
        s.and_then(|s| {
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

    Ok(features)
}
