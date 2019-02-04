use chrono::naive::NaiveDate;
use native_tls::TlsConnector;
use serde::{de::Error, Deserialize, Deserializer};
use std::{
    cmp::Ordering,
    collections::HashMap,
    fmt,
    io::{Read, Write},
    net::TcpStream,
    str::FromStr,
};
use toml;

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
    pub fn from_date(date: &str, channel: &str) -> Result<Self, String> {
        let path = format!("/dist/{}/channel-rust-{}.toml", date, channel);
        Manifest::from_url(&path)
    }

    pub fn from_url(path: &str) -> Result<Manifest, String> {
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
        let body = body(&response)?;
        let manifest = toml::from_str(&body).map_err(|e| e.to_string())?;
        Ok(manifest)
    }

    pub fn pkg_for_target(&self, pkg: &str, target: &str) -> Option<PackageInfo> {
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

    pub fn pkg_version(&self, name: &str) -> Option<Version> {
        let pkg = self.pkg.get(name)?;
        pkg.version.clone()
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
    #[serde(deserialize_with = "version_from_str")]
    pub version: Option<Version>,
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
    fn to_u8(&self) -> u8 {
        match self {
            Channel::Stable => 0,
            Channel::Beta => 1,
            Channel::Nightly => 2,
        }
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stable" | "" => Ok(Channel::Stable),
            "beta" => Ok(Channel::Beta),
            "nightly" => Ok(Channel::Nightly),
            _ => Err(String::from("wrong channel")),
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

impl FromStr for Commit {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let split: Vec<&str> = input
            .trim_matches(|c| c == '(' || c == ')')
            .splitn(2, ' ')
            .collect();
        Ok(Commit {
            hash: split[0].to_string(),
            date: NaiveDate::parse_from_str(split[1], "%Y-%m-%d").map_err(|e| e.to_string())?,
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

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} ({} {})",
            self.version,
            self.commit.hash,
            self.commit.date.format("%Y-%m-%d").to_string()
        )
    }
}

impl FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let split: Vec<&str> = s.splitn(2, ' ').collect();
        let (raw_version, commit) = (split[0], split[1]);
        let split: Vec<&str> = raw_version.split('-').collect();
        let (version, channel) = if split.len() == 2 {
            (split[0].to_string(), split[1])
        } else {
            (split[0].to_string(), "")
        };
        let commit = commit.parse()?;
        let channel = channel.parse()?;
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

fn version_from_str<'de, D>(deserializer: D) -> Result<Option<Version>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    Ok(if s.is_empty() {
        None
    } else {
        Some(s.parse().map_err(D::Error::custom)?)
    })
}

fn body(response: &[u8]) -> Result<&str, String> {
    let pos = response
        .windows(4)
        .position(|x| x == b"\r\n\r\n")
        .ok_or("Not search pattern")?;
    let body = &response[pos + 4..response.len()];
    std::str::from_utf8(&body).map_err(|e| e.to_string())
}

#[test]
fn test_body() {
    let response = b"HTTP/2.0 200 OK\r\nx-amz-bucket-region: us-west-1\r\nserver: AmazonS3\r\nx-cache: Miss from cloudfront\r\n\r\ntest message";
    assert_eq!(body(response), Ok("test message"));
    let response = b"\r\n\r\ntest message";
    assert_eq!(body(response), Ok("test message"));
    let response = b"\r\n\r\ntest message\r\n\r\ntest message";
    assert_eq!(body(response), Ok("test message\r\n\r\ntest message"));
}
