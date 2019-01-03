#[macro_use]
extern crate serde_derive;

use chrono::{naive::NaiveDate, Datelike, Duration, Local};
use native_tls::TlsConnector;
use regex::Regex;
use std::collections::HashMap;
use std::io::{self, Error, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::process::Command;
use toml::from_str;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    pub manifest_version: String,
    pub date: NaiveDate,
    pub pkg: HashMap<String, PackageTargets>,
    pub renames: HashMap<String, Rename>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct PackageTargets {
    pub version: String,
    pub target: HashMap<String, PackageInfo>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct PackageInfo {
    pub available: bool,
    pub url: Option<String>,
    pub hash: Option<String>,
    pub xz_url: Option<String>,
    pub xz_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Rename {
    pub to: String,
}

fn get_body(path: String) -> io::Result<Manifest> {
    let connector = TlsConnector::new().unwrap();
    let stream = TcpStream::connect("static.rust-lang.org:443")?;
    let mut stream = connector
        .connect("static.rust-lang.org", stream)
        .map_err(|e| Error::new(ErrorKind::NotConnected, e.to_string()))?;
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: static.rust-lang.org\r\n\r\n",
        path
    )
    .into_bytes();
    stream.write_all(&request)?;
    let mut response = vec![];
    stream.read_to_end(&mut response)?;
    let pos = response
        .windows(4)
        .position(|x| x == b"\r\n\r\n")
        .ok_or_else(|| Error::new(ErrorKind::Other, "wrong http"))?;
    let body = &response[pos + 4..response.len()];
    let body_str = String::from_utf8(body.to_vec())
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let manifest =
        from_str(&body_str).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    Ok(manifest)
}

fn get_target() -> io::Result<(String, String)> {
    let re_target = Regex::new(r"Default host: (\w.+?)\n")
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
    let re_date = Regex::new(r".*?rustc.+\(.+?(\d{4}-\d{2}-\d{2})\)")
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
    let command = Command::new("rustup")
        .arg("show")
        .output()
        .expect("failed to execute process");
    let command_str = String::from_utf8(command.stdout)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let target = re_target
        .captures(&command_str)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "regex not found target"))?;
    let date = re_date
        .captures(&command_str)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "regex not found date"))?;
    Ok((target[1].to_string(), date[1].to_string()))
}

fn get_components(target: &str) -> io::Result<Vec<String>> {
    let mut components = Vec::new();
    let re = Regex::new(&(format!(r"\n(\w.*?)\-{}\s\(installed\)", target)))
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
    let command = Command::new("rustup")
        .arg("component")
        .arg("list")
        .output()
        .expect("failed to execute process");
    let command_str = String::from_utf8(command.stdout)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    for cap in re.captures_iter(&command_str) {
        components.push(cap[1].to_string());
    }
    Ok(components)
}

fn get_date() -> io::Result<String> {
    let (target, date) = get_target()?;
    let components = get_components(&target)?;
    println!("Target: {}, date {}", target, date);
    println!("Components: {:?}", components);
    let naive_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let local_time = Local::today();
    let mut manifests = 0;
    for i in 0..31 {
        if let Some(new_time) = local_time.checked_sub_signed(Duration::days(i)) {
            let path = format!(
                "/dist/{}-{}-{}/channel-rust-nightly.toml",
                new_time.year(),
                new_time.month(),
                new_time.day()
            );
            if let Ok(manifest) = get_body(path) {
                let check = components.iter().all(|c| {
                    let component = if let Some(rename) = manifest.renames.get(c) {
                        rename.to.clone()
                    } else {
                        c.to_string()
                    };
                    if let Some(package_target) = manifest.pkg.get(&component) {
                        if let Some(package_info) = package_target.target.get(&target) {
                            package_info.available
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                });
                if check && manifests == 0 && new_time.naive_local() > naive_date {
                    return Ok(format!(
                        "use: rustup update (date - {}-{}-{})",
                        new_time.year(),
                        new_time.month(),
                        new_time.day()
                    ));
                } else if check {
                    return Ok(format!(
                        "{}-{}-{} - last build with all components",
                        new_time.year(),
                        new_time.month(),
                        new_time.day()
                    ));
                }
                manifests += 1;
            }
        }
    }
    Err(Error::new(
        ErrorKind::Other,
        "no found version with all components",
    ))
}

fn main() {
    match get_date() {
        Ok(text) => print!("{}", text),
        Err(text) => print!("error: {}", text),
    }
}
