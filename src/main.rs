#[macro_use]
extern crate serde_derive;

use chrono::{naive::NaiveDate, Duration, Local};
use native_tls::TlsConnector;
use serde::{de::Error, Deserialize, Deserializer};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::ops::Sub;
use std::process::Command;
use std::str::FromStr;
use toml::from_str;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
struct Toolchain {
    channel: String,
    target: String,
    version: Version,
    components: Vec<String>,
}

impl Toolchain {
    fn new() -> Result<Toolchain, String> {
        let (channel, target) = get_channel_target()?;
        let components = get_components(&target)?;
        let version = get_version()?;
        Ok(Toolchain {
            channel,
            target,
            version,
            components,
        })
    }

    fn info(&self) -> String {
        format!(
            "Installed: {}-{} {} ({} {})\n{}",
            self.channel,
            self.target,
            self.version.version,
            self.version.commit.hash,
            self.version.commit.date,
            match self.components.len() {
                0 => "With no components".to_string(),
                1 => format!("With component: {}", self.components[0]),
                _ => format!("With components: {}", print_vec(&self.components, ", ")),
            }
        )
    }
}

#[derive(Debug, Clone)]
pub struct Rust {
    offset: i64,
    date: NaiveDate,
    toolchain: Toolchain,
    manifest: Option<Manifest>,
}

impl Rust {
    pub fn new() -> Rust {
        let toolchain = Toolchain::new().unwrap();
        let date = Local::today().naive_local();
        let manifest =
            Manifest::from_date(&date.format("%Y-%m-%d").to_string(), &toolchain.channel);
        Rust {
            offset: -1,
            date,
            toolchain,
            manifest,
        }
    }

    pub fn from_date(date_str: &str) -> Rust {
        let toolchain = Toolchain::new().unwrap();
        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").unwrap();
        let offset = (Local::today().naive_local() - date).num_days() - 1;
        let manifest = Manifest::from_date(date_str, &toolchain.channel);
        Rust {
            offset,
            date,
            toolchain,
            manifest,
        }
    }

    pub fn missing_components(&self) -> Vec<String> {
        match &self.manifest {
            Some(manifest) => self
                .toolchain
                .components
                .iter()
                .filter(|&c| {
                    let component = match manifest.renames.get(c) {
                        Some(rename) => rename.to.clone(),
                        None => c.to_string(),
                    };
                    match manifest.get_pkg_for_target(&component, &self.toolchain.target) {
                        Some(package_info) => !package_info.available,
                        None => true,
                    }
                })
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn manifest_rust_version(&self) -> Option<Version> {
        match &self.manifest {
            Some(manifest) => manifest.get_rust_version().ok(),
            None => None,
        }
    }

    pub fn date_str(&self) -> String {
        self.date.format("%Y-%m-%d").to_string()
    }

    pub fn print_info(&self) {
        println!("{}", &self.toolchain.info());
    }
}

impl Default for Rust {
    fn default() -> Self {
        Self::new()
    }
}

impl Iterator for Rust {
    type Item = Rust;

    fn next(&mut self) -> Option<Self::Item> {
        self.offset += 1;
        self.date = Local::today()
            .naive_local()
            .sub(Duration::days(self.offset));
        self.manifest = Manifest::from_date(
            &self.date.format("%Y-%m-%d").to_string(),
            &self.toolchain.channel,
        );
        Some(self.clone())
    }
}

fn get_channel_target() -> Result<(String, String), String> {
    let command = Command::new("rustup")
        .arg("show")
        .arg("active-toolchain")
        .output()
        .expect("failed to execute process");
    let output = String::from_utf8(command.stdout).map_err(|e| e.to_string())?;
    let split: Vec<&str> = output.trim().splitn(2, '-').collect();
    let channel = split[0].to_string();
    let target = split[1].to_string();
    Ok((channel, target))
}

fn get_components(target: &str) -> Result<Vec<String>, String> {
    let command = Command::new("rustup")
        .arg("component")
        .arg("list")
        .output()
        .expect("failed to execute process");
    let output = String::from_utf8(command.stdout).map_err(|e| e.to_string())?;
    let split: Vec<&str> = output
        .split('\n')
        .filter(|&s| s.contains("(installed)"))
        .collect();
    let components: Vec<String> = split
        .iter()
        .map(|s| {
            s.replace(" (installed)", "")
                .replace(&format!("-{}", target), "")
        })
        .collect();
    Ok(components)
}

fn get_version() -> Result<Version, String> {
    let command = Command::new("rustc")
        .arg("-V")
        .output()
        .expect("failed to execute process");
    let output = String::from_utf8(command.stdout).map_err(|e| e.to_string())?;
    let version = Version::from_str(&output.replace("rustc ", ""))?;
    Ok(version)
}

fn print_vec(input: &[String], comma: &str) -> String {
    input
        .iter()
        .enumerate()
        .fold(String::new(), |mut acc, (i, s)| {
            if i > 0 {
                acc.push_str(comma);
            }
            acc.push_str(&s);
            acc
        })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    #[serde(deserialize_with = "u8_from_str")]
    pub manifest_version: u8,
    pub date: NaiveDate,
    pub pkg: HashMap<String, PackageTargets>,
    pub renames: HashMap<String, Rename>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackageTargets {
    pub version: String,
    pub target: HashMap<String, PackageInfo>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackageInfo {
    pub available: bool,
    pub url: Option<String>,
    pub hash: Option<String>,
    pub xz_url: Option<String>,
    pub xz_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Rename {
    pub to: String,
}

impl Manifest {
    pub fn from_date(date: &str, channel: &str) -> Option<Self> {
        let path = format!("/dist/{}/channel-rust-{}.toml", date, channel);
        fetch_manifest(&path).ok()
    }

    pub fn get_pkg_for_target(&self, pkg: &str, target: &str) -> Option<PackageInfo> {
        match self.pkg.get(pkg) {
            Some(package_target) => match package_target.target.get(target) {
                Some(package_info) => Some(package_info.clone()),
                None => match package_target.target.get("*") {
                    Some(package_info) => Some(package_info.clone()),
                    None => None,
                },
            },
            None => None,
        }
    }

    pub fn get_rust_version(&self) -> Result<Version, String> {
        let pkg_rust = self
            .pkg
            .get("rust")
            .ok_or("Manifest not contain pkg rust")?;
        Version::from_str(&pkg_rust.version)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "stable" | "" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            "nightly" => Some(Channel::Nightly),
            _ => None,
        }
    }

    fn to_u8(&self) -> u8 {
        match self {
            Channel::Stable => 0,
            Channel::Beta => 1,
            Channel::Nightly => 2,
        }
    }
}

impl PartialOrd for Channel {
    fn partial_cmp(&self, other: &Channel) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Channel {
    fn cmp(&self, other: &Channel) -> Ordering {
        self.to_u8().cmp(&other.to_u8())
    }
}

#[derive(Clone, Debug, Eq)]
pub struct Commit {
    pub hash: String,
    pub date: NaiveDate,
}

impl Commit {
    fn from(input: (&str, &str)) -> Result<Self, String> {
        Ok(Commit {
            hash: input.0.to_string(),
            date: NaiveDate::parse_from_str(input.1, "%Y-%m-%d").map_err(|e| e.to_string())?,
        })
    }
}

impl PartialOrd for Commit {
    fn partial_cmp(&self, other: &Commit) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Commit {
    fn cmp(&self, other: &Commit) -> Ordering {
        self.date.cmp(&other.date)
    }
}

impl PartialEq for Commit {
    fn eq(&self, other: &Commit) -> bool {
        self.date == other.date
    }
}

#[derive(Clone, Debug, Eq)]
pub struct Version {
    pub channel: Channel,
    pub version: String,
    pub commit: Commit,
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        match self.channel.cmp(&other.channel) {
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
            Ordering::Equal => match self.version.cmp(&other.version) {
                Ordering::Greater => Ordering::Greater,
                Ordering::Less => Ordering::Less,
                Ordering::Equal => match self.commit.cmp(&other.commit) {
                    Ordering::Greater => Ordering::Greater,
                    Ordering::Less => Ordering::Less,
                    Ordering::Equal => Ordering::Equal,
                },
            },
        }
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Version) -> bool {
        self.channel == other.channel
            && self.version == other.version
            && self.commit.date == other.commit.date
    }
}

impl FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let split: Vec<&str> = s
            .split_whitespace()
            .map(|w| w.trim_matches(|c| c == '(' || c == ')'))
            .collect();
        let (raw_version, hash, date) = (split[0], split[1], split[2]);
        let split: Vec<&str> = raw_version.split('-').collect();
        let (version, channel) = if split.len() == 2 {
            (split[0].to_string(), split[1])
        } else {
            (split[0].to_string(), "")
        };
        let commit = Commit::from((hash, date))?;
        let channel = Channel::from_str(channel).ok_or_else(|| "Wrong channel".to_string())?;
        Ok(Version {
            channel,
            version,
            commit,
        })
    }
}

fn u8_from_str<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    u8::from_str_radix(s, 10).map_err(D::Error::custom)
}

pub fn fetch_manifest(path: &str) -> Result<Manifest, String> {
    let connector = TlsConnector::new().map_err(|e| e.to_string())?;
    let stream = TcpStream::connect("static.rust-lang.org:443").map_err(|e| e.to_string())?;
    let mut stream = connector
        .connect("static.rust-lang.org", stream)
        .map_err(|e| e.to_string())?;
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: static.rust-lang.org\r\n\r\n",
        path
    )
    .into_bytes();
    stream.write_all(&request).map_err(|e| e.to_string())?;
    let mut response = vec![];
    stream
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    let body = get_body(&response)?;
    let manifest = from_str(&body).map_err(|e| e.to_string())?;
    Ok(manifest)
}

fn get_body(response: &[u8]) -> Result<&str, String> {
    let pos = response
        .windows(4)
        .position(|x| x == b"\r\n\r\n")
        .ok_or("Not search pattern")?;
    let body = &response[pos + 4..response.len()];
    std::str::from_utf8(&body).map_err(|e| e.to_string())
}

fn main() {
    let rust = Rust::new();
    rust.print_info();

    let v = rust
        .filter(|r| r.manifest.is_some() && r.missing_components().is_empty())
        .nth(0)
        .unwrap();

    println!("{:?}", v.toolchain.version);
    println!("{:?}", v.manifest_rust_version());
    println!(
        "{:?}",
        v.toolchain.version.cmp(&v.manifest_rust_version().unwrap())
    );

    // // match value {
    //     // Some(v) =>
    //     match (
    //         v.offset,
    //         v.toolchain.version > v.manifest.clone().unwrap().get_rust_version().unwrap(),
    //     ) {
    //         (0, true) => println!("Use: \"rustup update\" (new version from {})", v.date_str()),
    //         (0, false) => println!("Current version is up to date"),
    //         _ => println!(
    //             "Use: \"rustup default {}-{}\"{}",
    //             v.toolchain.channel,
    //             v.date_str(),
    //             match v.toolchain.components.len() {
    //                 0 => String::new(),
    //                 _ => format!(
    //                     "\n     \"rustup component add {}\"",
    //                     print_vec(&v.toolchain.components, " ")
    //                 ),
    //             }
    //         ),
    //     // },
    //     // None => println!("error: no found version with all components"),
    // }
}
