#[macro_use]
extern crate serde_derive;

mod manifest;

use chrono::{naive::NaiveDate, Duration, Local};
use std::ops::Sub;
use std::process::Command;

use crate::manifest::{Manifest, Version};

#[derive(Debug, Clone)]
struct Rust {
    channel: String,
    target: String,
    version: Version,
    components: Vec<String>,
}

impl Rust {
    fn new() -> Result<Rust, String> {
        let (channel, target) = get_channel_target()?;
        let components = get_components(&target)?;
        let version = get_version()?;
        Ok(Rust {
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

struct ManifestIter {
    date: NaiveDate,
    rust: Rust,
    manifest: Option<Manifest>
}

fn missing_components(rust: &Rust, manifest: &Manifest) -> Vec<String> {
    rust.components
        .iter()
        .filter(|&c| {
            let component = match manifest.renames.get(c) {
                Some(rename) => rename.to.clone(),
                None => c.to_string(),
            };
            match manifest.get_pkg_for_target(&component, &rust.target) {
                Some(package_info) => !package_info.available,
                None => true,
            }
        })
        .cloned()
        .collect()
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

#[derive(Debug, Clone)]
struct Value {
    offset: i64,
    days: i64,
    rust: Rust,
    date: NaiveDate,
    // date_str: String,
    manifest: Option<Manifest>,
}

impl Value {
    fn new() -> Value {
        let rust = Rust::new().unwrap();
        let date = Local::today().naive_local();
        let manifest = Manifest::from_date(&date.format("%Y-%m-%d").to_string(), &rust.channel);
        Value {
            offset: 0,
            days: 0,
            // date_str,
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
        if offset_date >= self.value.rust.version.commit.date {
            self.value.manifest = Manifest::from_date(&offset_date.format("%Y-%m-%d").to_string(), &self.value.rust.channel);
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
    // let value = meta
    // .filter(|v| {
    //     v.manifest.is_some()
    //         && v.rust
    //             .missing_components(&v.manifest.clone().unwrap())
    //             .is_empty()
    // })
    // ;
    for v in meta {
        println!(
            "{} {:?}",
            v.date.sub(Duration::days(v.offset)).format("%Y-%m-%d").to_string(),
            missing_components(&v.rust, &v.manifest.unwrap())
        );
    }

    // .nth(0);
    // match value {
    //     Some(v) => match v.days {
    //         0 => println!("Use: \"rustup update\" (new version from {})", v.date_str),
    //         _ => println!(
    //             "Use: \"rustup default {}-{}\"{}",
    //             v.rust.channel,
    //             v.date_str,
    //             match v.rust.components.len() {
    //                 0 => String::new(),
    //                 _ => format!(
    //                     "\n     \"rustup component add {}\"",
    //                     print_vec(&v.rust.components, " ")
    //                 ),
    //             }
    //         ),
    //     },
    //     None => println!("error: no found version with all components"),
    // }
}
