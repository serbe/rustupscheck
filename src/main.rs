#[macro_use]
extern crate serde_derive;

use chrono::{naive::NaiveDate, Duration, Local};
use native_tls::TlsConnector;
use regex::Regex;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::ops::Sub;
use std::process::Command;
use toml::from_str;

#[derive(Debug, Clone)]
struct Rust {
    channel: String,
    target: String,
    version: String,
    date: String,
    naive_date: NaiveDate,
    components: Vec<String>,
}

impl Rust {
    fn new() -> Option<Rust> {
        let re = Regex::new(r"(stable|beta|nightly)-(\w.+?)\n").ok()?;
        let command = Command::new("rustup")
            .arg("show")
            .arg("active-toolchain")
            .output()
            .expect("failed to execute process");
        let command_str = String::from_utf8(command.stdout).ok()?;
        let cap = re.captures(&command_str)?;
        let (channel, target) = (cap[1].to_string(), cap[2].to_string());
        let re = Regex::new(r"rustc\s(\d.+?\d)[\-\s].+?(\d{4}-\d{2}-\d{2})").ok()?;
        let command = Command::new("rustc")
            .arg("-V")
            .output()
            .expect("failed to execute process");
        let command_str = String::from_utf8(command.stdout).ok()?;
        let cap = re.captures(&command_str)?;
        let (version, date) = (cap[1].to_string(), cap[2].to_string());
        let naive_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()?;
        let mut components = Vec::new();
        let re = Regex::new(&(format!(r"\n(\w.*?)\-{}\s\(installed\)", target))).ok()?;
        let command = Command::new("rustup")
            .arg("component")
            .arg("list")
            .output()
            .expect("failed to execute process");
        let command_str = String::from_utf8(command.stdout).ok()?;
        for cap in re.captures_iter(&command_str) {
            components.push(cap[1].to_string());
        }
        Some(Rust {
            channel,
            target,
            version,
            date,
            naive_date,
            components,
        })
    }

    fn missing_components(&self, manifest: &Manifest) -> Vec<String> {
        self.components
            .iter()
            .filter(|&c| {
                let component = match manifest.renames.get(c) {
                    Some(rename) => rename.to.clone(),
                    None => c.to_string(),
                };
                match manifest.pkg.get(&component) {
                    Some(package_target) => match package_target.target.get(&self.target) {
                        Some(package_info) => !package_info.available,
                        _ => true,
                    },
                    None => true,
                }
            })
            .cloned()
            .collect()
    }

    fn info(&self) -> String {
        format!(
            "Installed: {}-{} {} ({})\n{}",
            self.channel,
            self.target,
            self.version,
            self.date,
            match self.components.len() {
                0 => "With no components".to_string(),
                1 => format!("With component: {}", self.components[0]),
                _ => format!("With components: {}", print_vec(&self.components, ", ")),
            }
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Manifest {
    manifest_version: String,
    date: NaiveDate,
    pkg: HashMap<String, PackageTargets>,
    renames: HashMap<String, Rename>,
}

#[derive(Clone, Debug, Deserialize)]
struct PackageTargets {
    version: String,
    target: HashMap<String, PackageInfo>,
}

#[derive(Clone, Debug, Deserialize)]
struct PackageInfo {
    available: bool,
    url: Option<String>,
    hash: Option<String>,
    xz_url: Option<String>,
    xz_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Rename {
    to: String,
}

#[derive(Debug, Clone)]
struct Value {
    offset: i64,
    days: i64,
    rust: Rust,
    date: NaiveDate,
    date_str: String,
    manifest: Option<Manifest>,
}

impl Value {
    fn new() -> Value {
        let rust = Rust::new().unwrap();
        let date = Local::today().naive_local();
        let date_str = date.sub(Duration::days(0)).format("%Y-%m-%d").to_string();
        let path = format!("/dist/{}/channel-rust-{}.toml", date_str, rust.channel);
        let manifest = fetch_manifest(path);
        Value {
            offset: 0,
            days: 0,
            date_str,
            rust,
            date,
            manifest,
        }
    }
}

struct Meta {
    value: Value,
}

impl Meta {
    fn new() -> Meta {
        Meta {
            value: Value::new(),
        }
    }

    fn print_info(&self) {
        println!("{}", self.value.rust.info());
    }
}

impl Iterator for Meta {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        self.value.offset += 1;

        let offset_date = self.value.date.sub(Duration::days(self.value.offset));
        if offset_date >= self.value.rust.naive_date {
            self.value.date_str = offset_date.format("%Y-%m-%d").to_string();
            let path = format!(
                "/dist/{}/channel-rust-{}.toml",
                self.value.date_str, self.value.rust.channel
            );
            self.value.manifest = fetch_manifest(path);
            if self.value.manifest.is_some() {
                self.value.days += 1;
            }
            Some(self.value.clone())
        } else {
            None
        }
    }
}

fn fetch_manifest(path: String) -> Option<Manifest> {
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
    let pos = response.windows(4).position(|x| x == b"\r\n\r\n")?;
    let body = &response[pos + 4..response.len()];
    let body_str = String::from_utf8(body.to_vec()).ok()?;
    let manifest = from_str(&body_str).ok()?;
    Some(manifest)
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

fn main() {
    let meta = Meta::new();
    meta.print_info();
    let value = meta
        .filter(|v| {
            v.manifest.is_some()
                && v.rust
                    .missing_components(&v.manifest.clone().unwrap())
                    .is_empty()
        })
        .nth(0);
    match value {
        Some(v) => match v.days {
            0 => println!("Use: \"rustup update\" (new version from {})", v.date_str),
            _ => println!(
                "Use: \"rustup default {}-{}\"{}",
                v.rust.channel,
                v.date_str,
                match v.rust.components.len() {
                    0 => String::new(),
                    _ => format!(
                        "\n     \"rustup component add {}\"",
                        print_vec(&v.rust.components, " ")
                    ),
                }
            ),
        },
        None => println!("error: no found version with all components"),
    }
}
