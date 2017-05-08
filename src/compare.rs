use errors::*;

pub struct Config {
    pub toolchain1: String,
    pub toolchain2: String,
    pub target: String,
    pub mode: Mode,
    pub rustflags: Option<String>,
    pub crates: Vec<Crate>,
}

pub enum Mode {
    Debug,
    Release,
}

pub enum Crate {
    Repo(String, String), // url, relative path to Cargo.toml file
    Registry(String, String), // name, version
}

pub fn load_crate_list(path: &str) -> Result<Vec<Crate>> {
    panic!()
}

pub fn run(config: &Config) -> Result<()> {
    panic!()
}
