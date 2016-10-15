use errors::*;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write, BufRead, BufReader};
use std::path::Path;

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

pub fn append(path: &Path, s: &str) -> Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(s.as_bytes())?;
    Ok(())
}
