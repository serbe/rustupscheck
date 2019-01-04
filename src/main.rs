#[macro_use]
extern crate serde_derive;

use chrono::{naive::NaiveDate, Duration, Local};
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

fn get_toolchain() -> io::Result<(String, String, String, String)> {
    let re_target = Regex::new(r"Default host: (\w.+?)\n")
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
    let re = Regex::new(r".*?rustc\s(\d.+?)\-(\w+?)\s\(.+?(\d{4}-\d{2}-\d{2})\)")
        .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
    let command = Command::new("rustup")
        .arg("show")
        .output()
        .expect("failed to execute process");
    let command_str = String::from_utf8(command.stdout)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let target = re_target
        .captures(&command_str)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "regex not found target"))?[1]
        .to_string();
    let cap = re
        .captures(&command_str)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "regex not found date"))?;
    let (channel, version, date) = (cap[2].to_string(), cap[1].to_string(), cap[3].to_string());
    Ok((target, channel, version, date))
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

fn print_vec(input: &[String]) -> String {
    input
        .iter()
        .enumerate()
        .fold(String::new(), |mut acc, (i, s)| {
            if i > 0 {
                acc.push_str(", ");
            }
            acc.push_str(&s);
            acc
        })
}

fn get_date() -> io::Result<String> {
    let (target, channel, version, date) = get_toolchain()?;
    let components = get_components(&target)?;
    println!("Installed: {}-{} {} ({})", channel, target, version, date);
    println!("With components: {}", print_vec(&components));
    let naive_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    let local_time = Local::today();
    let mut manifests = 0;
    for i in 0..31 {
        if let Some(new_time) = local_time.checked_sub_signed(Duration::days(i)) {
            let date_str = new_time.format("%Y-%m-%d").to_string();
            let path = format!("/dist/{}/channel-rust-{}.toml", date_str, channel);
            if let Ok(manifest) = get_body(path) {
                let check: Vec<String> = components
                    .iter()
                    .filter(|&c| {
                        let component = if let Some(rename) = manifest.renames.get(c) {
                            rename.to.clone()
                        } else {
                            c.to_string()
                        };
                        if let Some(package_target) = manifest.pkg.get(&component) {
                            if let Some(package_info) = package_target.target.get(&target) {
                                !package_info.available
                            } else {
                                true
                            }
                        } else {
                            true
                        }
                    })
                    .cloned()
                    .collect();
                if check.is_empty() && manifests == 0 && new_time.naive_local() > naive_date {
                    return Ok(format!(
                        "Use: \"rustup update\" (new version from {})",
                        date_str
                    ));
                } else if check.is_empty() {
                    return Ok(format!("Use: \"rustup default {}-{}\"", channel, date_str));
                }
                println!(
                    "Build {} not have components: {}",
                    date_str,
                    print_vec(&check)
                );
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
        Ok(text) => println!("{}", text),
        Err(text) => println!("error: {}", text),
    }
}
