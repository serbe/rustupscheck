#[macro_use]
extern crate serde_derive;

mod manifest;

use chrono::{naive::NaiveDate, Duration, Local};
use regex::Regex;
use std::ops::Sub;
use std::process::Command;

use crate::manifest::{Manifest, fetch_manifest};

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
        let command = Command::new("rustup")
            .arg("show")
            .arg("active-toolchain")
            .output()
            .expect("failed to execute process");
        let (channel, target) = get_channel_target(&String::from_utf8(command.stdout).ok()?)?;
        let command = Command::new("rustc")
            .arg("-V")
            .output()
            .expect("failed to execute process");
        let (version, date) = get_version_date(&String::from_utf8(command.stdout).ok()?)?;
        let naive_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").ok()?;
        let command = Command::new("rustup")
            .arg("component")
            .arg("list")
            .output()
            .expect("failed to execute process");
        let components = get_components(&String::from_utf8(command.stdout).ok()?, &target)?;
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

fn get_channel_target(s: &str) -> Option<(String, String)> {
    let re = Regex::new(r"(stable|beta|nightly)-(\w.+?)\n").ok()?;
    let cap = re.captures(&s)?;
    Some((cap[1].to_string(), cap[2].to_string()))
}

fn get_version_date(s: &str) -> Option<(String, String)> {
    let re = Regex::new(r"rustc\s(\d.+?\d)[\-\s].+?(\d{4}-\d{2}-\d{2})").ok()?;
    let cap = re.captures(&s)?;
    Some((cap[1].to_string(), cap[2].to_string()))
}

fn get_components(s: &str, target: &str) -> Option<Vec<String>> {
    let mut components = Vec::new();
    let re = Regex::new(&(format!(r"\n(\w.*?)\-{}\s\(installed\)", target))).ok()?;
    for cap in re.captures_iter(&s) {
        components.push(cap[1].to_string());
    }
    Some(components)
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
        let manifest = fetch_manifest(&path);
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
            self.value.manifest = fetch_manifest(&path);
            if self.value.manifest.is_some() {
                self.value.days += 1;
            }
            Some(self.value.clone())
        } else {
            None
        }
    }
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
