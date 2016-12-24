use errors::*;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write, BufRead, BufReader};
use std::path::Path;
use serde::{Serialize, Deserialize};
use serde_json;

pub fn write_string(path: &Path, s: &str) -> Result<()> {
    let mut f = File::create(path)?;
    f.write_all(s.as_bytes())?;
    Ok(())
}

pub fn read_string(path: &Path) -> Result<String> {
    let mut f = BufReader::new(File::open(path)?);
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    Ok(buf)
}

pub fn write_lines(path: &Path, lines: &[String]) -> Result<()> {
    write_string(path, &(lines.join("\n") + "\n"))
}

pub fn read_lines(path: &Path) -> Result<Vec<String>> {
    let contents = read_string(path)?;
    Ok(contents.lines()
       .map(|l| l.to_string())
       .filter(|l| !l.chars().all(|c| c.is_whitespace()))
       .collect())
}

pub fn append_line(path: &Path, s: &str) -> Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(s.as_bytes())?;
    f.write_all("\n".as_bytes())?;
    Ok(())
}

pub fn write_json<T>(path: &Path, t: &T) -> Result<()>
    where T: Serialize + Deserialize
{
    let ref s = serde_json::to_string(t)?;
    write_string(path, s)
}

pub fn read_json<T>(path: &Path, t: &T) -> Result<()> {
    let ref s = read_string(path)?;
    let t = serde_json::from_str(s)?;
    Ok(t)
}
