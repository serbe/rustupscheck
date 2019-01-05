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
                0 => format!("With no components"),
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

struct Meta {
    value: Value
}

impl Meta {
    fn new() -> Meta {
        Meta {
            value: Value::new()
        }
    }
}

impl Iterator for Meta {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        self.value.offset += 1;

        let date_str = self.value.date
            .sub(Duration::days(self.value.offset))
            .format("%Y-%m-%d")
            .to_string();
        let path = format!("/dist/{}/channel-rust-{}.toml", date_str, self.value.rust.channel);
        self.value.manifest = fetch_manifest(path);
        Some(self.value.clone())
    }
}

#[derive(Debug, Clone)]
struct Value {
    offset: i64,
    rust: Rust,
    date: NaiveDate,
    manifest: Option<Manifest>,
}

impl Value {
    fn new() -> Value {
        let offset = 0;
        let rust = Rust::new().unwrap();
        let date = Local::today().naive_local();
        let date_str = date
            .sub(Duration::days(offset))
            .format("%Y-%m-%d")
            .to_string();
        let path = format!("/dist/{}/channel-rust-{}.toml", date_str, rust.channel);
        let manifest = fetch_manifest(path);
        Value {
            offset,
            rust,
            date,
            manifest,
        }
    }
}

fn fetch_manifest(path: String) -> Option<Manifest> {
    let connector = TlsConnector::new().unwrap();
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

fn get_rust_date(input: Option<&PackageTargets>) -> Option<NaiveDate> {
    match input {
        Some(rust) => {
            let re_date = Regex::new(r".+?(\d{4}-\d{2}-\d{2})").ok()?;
            let date = re_date.captures(&rust.version)?[1].to_string();
            NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()
        }
        None => None,
    }
}

fn get_date() -> Option<String> {
    let rust = Rust::new()?;
    println!("{}", &rust.info());
    let naive_date = NaiveDate::parse_from_str(&rust.date, "%Y-%m-%d").ok()?;
    let local_time = Local::today();
    // let it = (0..31).into_iter().map(|i| {
    //     let new_time = local_time
    //         .sub(Duration::days(i))
    //         .format("%Y-%m-%d")
    //         .to_string();
    //     let path = format!("/dist/{}/channel-rust-{}.toml", new_time, rust.channel);
    //     match fetch_manifest(path) {
    //         Some(manifest) => {
    //             let rust_date = get_rust_date(manifest.pkg.get("rust"))?;
    //             let missing_components = &rust.missing_components(&manifest);
    //             Some((rust_date, missing_components))
    //         }
    //         _ => None,
    //     }
    // });
    let mut manifests = 0;
    for i in 0..31 {
        let date_str = local_time
            .sub(Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        let path = format!("/dist/{}/channel-rust-{}.toml", date_str, rust.channel);
        if let Some(manifest) = fetch_manifest(path) {
            let rust_date = get_rust_date(manifest.pkg.get("rust"))?;
            let missing_components = &rust.missing_components(&manifest);
            if missing_components.is_empty() && manifests == 0 && rust_date > naive_date {
                return Some(format!(
                    "Use: \"rustup update\" (new version from {})",
                    date_str
                ));
            } else if missing_components.is_empty() && rust_date > naive_date {
                return Some(format!(
                    "Use: \"rustup default {}-{}\"\n     \"rustup component add {}\"",
                    rust.channel,
                    date_str,
                    print_vec(&rust.components, " ")
                ));
            } else if missing_components.is_empty() {
                return Some("Updates not found".to_string());
            }
            println!(
                "Build {} not have components: {}",
                date_str,
                print_vec(&missing_components, ", ")
            );
            manifests += 1;
        }
    }
    None
}

fn main() {
    // match get_date() {
    //     Some(text) => println!("{}", text),
    //     None => println!("error: no found version with all components"),
    // }
    let meta = Meta::new();
    println!("{:?}", meta.value)
}
