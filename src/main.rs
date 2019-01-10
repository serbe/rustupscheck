#[macro_use]
extern crate serde_derive;

mod manifest;

use chrono::{naive::NaiveDate, Duration, Local};
use std::ops::Sub;
use std::process::Command;

use crate::manifest::{Manifest, Version};

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
            toolchain,
            date,
            manifest,
        }
    }

    pub fn missing_components(&self) -> Option<Vec<String>> {
        match &self.manifest {
            Some(manifest) => Some(
                self.toolchain
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
            ),
            None => None,
        }
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

// impl IntoIterator for Rust {
//     type Item = Rust;
//     type IntoIter = Iterator::Rust;

//     fn into_iter(self) -> Self::IntoIter {
//         ManifestIter {
//             date: self.date,
//             toolchain: Toolchain::new().unwrap(),
//             manifest: Some(self),
//         }
//     }
// }

impl Iterator for Rust {
    type Item = Rust;

    fn next(&mut self) -> Option<Self::Item> {
        let old = self.clone();
        self.date = self.date.sub(Duration::days(1));
        self.manifest = Manifest::from_date(
            &self.date.format("%Y-%m-%d").to_string(),
            &self.toolchain.channel,
        );
        Some(old)
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

// impl Iterator for Meta {
//     type Item = Value;

//     fn next(&mut self) -> Option<Value> {
//         self.value.offset += 1;

//         let offset_date = self.value.date.sub(Duration::days(self.value.offset));
//         if offset_date >= self.value.toolchain.version.commit.date {
//             self.value.manifest = Manifest::from_date(
//                 &offset_date.format("%Y-%m-%d").to_string(),
//                 &self.value.toolchain.channel,
//             );
//             if self.value.manifest.is_some() {
//                 self.value.days += 1;
//             }
//             Some(self.value.clone())
//         } else {
//             None
//         }
//     }
// }

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
    let rust = Rust::new();
    rust.print_info();

    for r in rust.take(2) {
        println!("{}", r.date.format("%Y-%m-%d").to_string(),);
    }
    // let value = meta
    // .filter(|v| {
    //     v.manifest.is_some()
    //         && v.rust
    //             .missing_components(&v.manifest.clone().unwrap())
    //             .is_empty()
    // })
    // ;
    // for v in meta {
    //     println!(
    //         "{} {:?}",
    //         v.date
    //             .sub(Duration::days(v.offset))
    //             .format("%Y-%m-%d")
    //             .to_string(),
    //         missing_components(&v.toolchain, &v.manifest.unwrap())
    //     );
    // }

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
