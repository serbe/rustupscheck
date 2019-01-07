use chrono::naive::NaiveDate;
use native_tls::TlsConnector;
use regex::Regex;
use serde::{de::Error, Deserialize, Deserializer};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use toml::from_str;

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
    pub fn get_rust_version(&self) -> Option<Version> {
        let pkg_rust = self.pkg.get("rust")?;
        Version::from_str(&pkg_rust.version)
    }
}

#[derive(Debug, PartialEq)]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            "nightly" => Some(Channel::Nightly),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Commit {
    hash: String,
    date: NaiveDate,
}

impl Commit {
    fn from(input: (String, String)) -> Option<Self> {
        Some(Commit {
            hash: input.0,
            date: NaiveDate::parse_from_str(&input.1, "%Y-%m-%d").ok()?,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct Version {
    channel: Channel,
    version: String,
    commit: Commit,
}

impl Version {
    fn from_str(s: &str) -> Option<Self> {
        let re = Regex::new(r"(?P<v>\d{1,2}(?:\.\d{1,2})+)\-?(?P<c>(?:(?<!-)|(?<=-)stable|(?<=-)beta|(?<=-)nightly))\s\((?P<h>\w.+)\s(?P<d>\d{4}(?:\-\d{2}){2})\)").ok()?;
        let c = re.captures(&s)?;
        let (version, channel, hash, date) = (
            c["v"].to_string(),
            c["c"].to_string(),
            c["h"].to_string(),
            c["d"].to_string(),
        );
        let commit = Commit::from((hash, date))?;
        let channel = Channel::from_str(&channel)?;
        Some(Version {
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

pub fn fetch_manifest(path: &str) -> Option<Manifest> {
    let connector = TlsConnector::new().ok()?;
    let stream = TcpStream::connect("static.rust-lang.org:443").ok()?;
    let mut stream = connector.connect("static.rust-lang.org", stream).ok()?;
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: static.rust-lang.org\r\n\r\n",
        path
    )
    .into_bytes();
    stream.write_all(&request).ok()?;
    let mut response = vec![];
    stream.read_to_end(&mut response).ok()?;
    let body = get_body(&response)?;
    let manifest = from_str(&body).ok()?;
    Some(manifest)
}

fn get_body(response: &[u8]) -> Option<&str> {
    let pos = response.windows(4).position(|x| x == b"\r\n\r\n")?;
    let body = &response[pos + 4..response.len()];
    std::str::from_utf8(&body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body() {
        let response = b"HTTP/2.0 200 OK\r\nx-amz-bucket-region: us-west-1\r\nserver: AmazonS3\r\nx-cache: Miss from cloudfront\r\n\r\ntest message";
        assert_eq!(get_body(response), Some("test message"));
        let response = b"\r\n\r\ntest message";
        assert_eq!(get_body(response), Some("test message"));
        let response = b"\r\n\r\ntest message\r\n\r\ntest message";
        assert_eq!(get_body(response), Some("test message\r\n\r\ntest message"));
    }

    #[test]
    fn wrong_path() {
        let path = "https://static.rust-lang.org/dist/01-01-2019/channel-rust-nightly.toml";
        let manifest = fetch_manifest(path);
        assert!(manifest.is_none());
        let path = "static.rust-lang.org";
        let manifest = fetch_manifest(path);
        assert!(manifest.is_none());
    }

    #[test]
    fn new_year_manifest() {
        let path = "https://static.rust-lang.org/dist/2019-01-01/channel-rust-nightly.toml";
        let optional_manifest = fetch_manifest(path);
        assert!(optional_manifest.is_some());
        let manifest = optional_manifest.unwrap();
        assert_eq!(manifest.manifest_version, 2u8);
        assert_eq!(
            Ok(manifest.date),
            NaiveDate::parse_from_str(&"2019-01-01", "%Y-%m-%d")
        );
        assert_eq!(
            manifest.renames.get("rls").unwrap().to,
            "rls-preview".to_string()
        );
        let rust1330 = Version {
            channel: Channel::Nightly,
            version: "1.33.0".to_string(),
            commit: Commit {
                hash: "9eac38634".to_string(),
                date: NaiveDate::parse_from_str(&"2018-12-31", "%Y-%m-%d").unwrap(),
            },
        };
        assert_eq!(manifest.get_rust_version(), Some(rust1330));
    }
}
