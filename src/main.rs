#[macro_use]
extern crate serde_derive;

use chrono::{naive::NaiveDate, Duration, Local};
use native_tls::TlsConnector;
use serde::{de::Error, Deserialize, Deserializer};
use std::{
    cmp::Ordering,
    collections::HashMap,
    env,
    fs::File,
    io::{Read, Write},
    net::TcpStream,
    ops::Sub,
    path::PathBuf,
    str::FromStr,
};
use toml::from_str;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
struct Component {
    name: String,
    required: bool,
    version: Version,
}

impl Component {
    fn from(manifest: &Manifest, name: &str) -> Self {
        let required = match name {
            "rustc" | "cargo" => true,
            _ => false,
        };
        let version = manifest.get_pkg_version(name).unwrap();
        Component {
            name: name.to_string(),
            required,
            version,
        }
    }
}

#[derive(Debug, Clone)]
struct Toolchain {
    channel: String,
    target: String,
    components: Vec<Component>,
    manifest: Manifest,
}

impl Toolchain {
    fn new() -> Result<Toolchain, String> {
        let (channel, target) = get_channel_target()?;
        let manifest = get_manifest()?;
        let components = get_components(&target)?
            .iter()
            .map(|s| Component::from(&manifest, s))
            .collect();
        Ok(Toolchain {
            channel,
            target,
            components,
            manifest,
        })
    }

    fn component_list(&self) -> Vec<String> {
        self.components
            .iter()
            .filter(|c| !c.required)
            .map(|c| c.name.to_string())
            .collect()
    }

    fn info(&self) -> String {
        let version = self.manifest.get_pkg_version("rustc").unwrap();
        format!(
            "Installed: {}-{} {} ({} {})\n{}",
            self.channel,
            self.target,
            version.version,
            version.commit.hash,
            version.commit.date,
            match self.component_list().len() {
                0 => "With no components".to_string(),
                1 => format!("With component: {}", self.component_list()[0]),
                _ => format!(
                    "With components: {}",
                    print_vec(&self.component_list(), ", ")
                ),
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
                .map(|c| &c.name)
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

    pub fn manifest_pkg_version(&self, name: &str) -> Option<Version> {
        match &self.manifest {
            Some(manifest) => manifest.get_pkg_version(name).ok(),
            None => None,
        }
    }

    pub fn date_str(&self) -> String {
        self.date.format("%Y-%m-%d").to_string()
    }

    pub fn print_info(&self) {
        println!("{}", &self.toolchain.info());
    }

    fn update_info(&self) -> Option<Vec<Component>> {
        if self.missing_components().is_empty() {
            Some(self.toolchain.components.iter().filter(|c| c.version != self.manifest.clone().unwrap().get_pkg_version(&c.name).unwrap()).cloned().collect())
        } else {
            None
        }
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
    let toolchain = env::var("RUSTUP_TOOLCHAIN").map_err(|e| e.to_string())?;
    let split: Vec<&str> = toolchain.splitn(2, '-').collect();
    let channel = split[0].to_string();
    let target = split[1].to_string();
    Ok((channel, target))
}

fn get_components(target: &str) -> Result<Vec<String>, String> {
    let rustup_home = env::var("RUSTUP_HOME").map_err(|e| e.to_string())?;
    let toolchain = env::var("RUSTUP_TOOLCHAIN").map_err(|e| e.to_string())?;
    let mut path = PathBuf::from(rustup_home);
    path.push("toolchains");
    path.push(toolchain);
    path.push("lib");
    path.push("rustlib");
    path.push("components");
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;
    let components: Vec<String> = contents
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(|s| s.replace(&format!("-{}", target), ""))
        .collect();
    Ok(components)
}

fn get_manifest() -> Result<Manifest, String> {
    let rustup_home = env::var("RUSTUP_HOME").unwrap();
    let toolchain = env::var("RUSTUP_TOOLCHAIN").unwrap();
    let mut path = PathBuf::from(rustup_home);
    path.push("toolchains");
    path.push(toolchain);
    path.push("lib");
    path.push("rustlib");
    path.push("multirust-channel-manifest");
    path.set_extension("toml");
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;
    let manifest: Manifest = toml::from_str(&contents).map_err(|e| e.to_string())?;
    Ok(manifest)
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

#[derive(Debug, Clone, Deserialize, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    #[serde(deserialize_with = "u8_from_str")]
    pub manifest_version: u8,
    pub date: NaiveDate,
    pub pkg: HashMap<String, PackageTargets>,
    pub renames: HashMap<String, Rename>,
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

    pub fn get_pkg_version(&self, name: &str) -> Result<Version, String> {
        let pkg = self
            .pkg
            .get(name)
            .ok_or_else(|| format!("Manifest not contain pkg {}", name))?;
        Version::from_str(&pkg.version)
    }
}

impl PartialEq for Manifest {
    fn eq(&self, other: &Manifest) -> bool {
        self.manifest_version == other.manifest_version
            && self.date == other.date
            && self.pkg == other.pkg
            && self.renames == other.renames
    }
}

#[derive(Clone, Debug, Deserialize, Eq)]
pub struct PackageTargets {
    pub version: String,
    pub target: HashMap<String, PackageInfo>,
}

impl PartialEq for PackageTargets {
    fn eq(&self, other: &PackageTargets) -> bool {
        self.version == other.version && self.target == other.target
    }
}

#[derive(Clone, Debug, Deserialize, Eq)]
pub struct PackageInfo {
    pub available: bool,
    pub url: Option<String>,
    pub hash: Option<String>,
    pub xz_url: Option<String>,
    pub xz_hash: Option<String>,
}

impl PartialEq for PackageInfo {
    fn eq(&self, other: &PackageInfo) -> bool {
        self.available == other.available
            && self.url == other.url
            && self.hash == other.hash
            && self.xz_url == other.xz_url
            && self.xz_hash == other.xz_hash
    }
}

#[derive(Clone, Debug, Deserialize, Eq)]
pub struct Rename {
    pub to: String,
}

impl PartialEq for Rename {
    fn eq(&self, other: &Rename) -> bool {
        self.to == other.to
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

    match (
        v.offset,
        v.toolchain.manifest.get_pkg_version("rust").ok() < v.manifest_pkg_version("rust"),
    ) {
        (0, true) => println!("Use: \"rustup update\" (new version from {})", v.date_str()),
        (0, false) => println!("Current version is up to date"),
        _ => println!(
            "Use: \"rustup default {}-{}\"{}",
            v.toolchain.channel,
            v.date_str(),
            match v.toolchain.components.len() {
                0 => String::new(),
                _ => format!(
                    "\n     \"rustup component add {}\"",
                    print_vec(&v.toolchain.component_list(), " ")
                ),
            }
        ),
    }
}
