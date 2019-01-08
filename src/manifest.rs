use chrono::naive::NaiveDate;
use native_tls::TlsConnector;
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
    pub fn get_rust_version(&self) -> Result<Version, String> {
        let pkg_rust = self
            .pkg
            .get("rust")
            .ok_or("Manifest not contain pkg rust")?;
        Version::from_str(&pkg_rust.version)
    }
}

#[derive(Clone, Debug, PartialEq)]
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
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub channel: Channel,
    pub version: String,
    pub commit: Commit,
}

impl Version {
    pub fn from_str(s: &str) -> Result<Self, String> {
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
        let channel = Channel::from_str(channel).ok_or("Wrong channel".to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body() {
        let response = b"HTTP/2.0 200 OK\r\nx-amz-bucket-region: us-west-1\r\nserver: AmazonS3\r\nx-cache: Miss from cloudfront\r\n\r\ntest message";
        assert_eq!(get_body(response), Ok("test message"));
        let response = b"\r\n\r\ntest message";
        assert_eq!(get_body(response), Ok("test message"));
        let response = b"\r\n\r\ntest message\r\n\r\ntest message";
        assert_eq!(get_body(response), Ok("test message\r\n\r\ntest message"));
    }

    #[test]
    fn wrong_path() {
        let path = "/dist/01-01-2019/channel-rust-nightly.toml";
        let manifest = fetch_manifest(path);
        assert!(manifest.is_err());
        let path = "static.rust-lang.org";
        let manifest = fetch_manifest(path);
        assert!(manifest.is_err());
    }

    #[test]
    fn new_year_manifest() {
        let path = "/dist/2019-01-01/channel-rust-nightly.toml";
        let optional_manifest = fetch_manifest(path);
        assert!(optional_manifest.is_ok());
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
        assert_eq!(manifest.get_rust_version(), Ok(rust1330));
        let rust_src = manifest.pkg.get("rust-src").unwrap();
        let target_info = rust_src.target.get("x86_64-pc-windows-gnu");
        assert!(target_info.is_none());
        let target_info = rust_src.target.get("*");
        assert!(target_info.is_some());
        let target_info = target_info.unwrap();
        assert_eq!(target_info.available, true);
    }

    #[test]
    fn parse_version() {
        let s = "rustc 1.33.0-nightly (9eac38634 2018-12-31)";
        let s = s.replace("rustc ", "");
        let split: Vec<&str> = s
            .split_whitespace()
            .map(|w| w.trim_matches(|c| c == '(' || c == ')'))
            .collect();
        assert_eq!(split.len(), 3);
        let (raw_version, hash, date) = (split[0], split[1], split[2]);
        assert_eq!(raw_version, "1.33.0-nightly");
        let split: Vec<&str> = raw_version.split('-').collect();
        let (version, channel) = if split.len() == 2 {
            (split[0], split[1])
        } else {
            (split[0], "")
        };
        assert_eq!(version, "1.33.0");
        assert_eq!(channel, "nightly");
        assert_eq!(hash, "9eac38634");
        assert_eq!(date, "2018-12-31");
        let commit = Commit::from((&hash, &date)).unwrap();
        assert_eq!(
            commit,
            Commit {
                hash: "9eac38634".to_string(),
                date: NaiveDate::parse_from_str(&"2018-12-31", "%Y-%m-%d").unwrap(),
            }
        );
        let channel = Channel::from_str(&channel).unwrap();
        assert_eq!(channel, Channel::Nightly);
        let ver = Version::from_str(&s).unwrap();
        assert_eq!(ver, Version{version: version.to_string(), channel, commit});
    }

    #[test]
    fn parse_active_toolchain() {
        let output = "nightly-x86_64-pc-windows-gnu\n";
        let split: Vec<&str> = output.trim().splitn(2, '-').collect();
        let channel = split[0];
        let target = split[1];
        assert_eq!(channel, "nightly");
        assert_eq!(target, "x86_64-pc-windows-gnu");
        let output = "rust-src (installed)\nrust-std-x86_64-unknown-redox\nrustc-x86_64-pc-windows-gnu (default)\nrustfmt-x86_64-pc-windows-gnu (installed)\n";
        let split: Vec<&str> = output
            .split('\n')
            .filter(|&s| s.contains("(installed)"))
            .collect();
        assert!(split.len() == 2);
        let components: Vec<String> = split
            .iter()
            .map(|s| {
                s.replace(" (installed)", "")
                    .replace(&format!("-{}", target), "")
            })
            .collect();
        assert_eq!(&components[0], "rust-src");
        assert_eq!(&components[1], "rustfmt");
    }
}
