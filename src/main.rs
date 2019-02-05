#[macro_use]
extern crate serde_derive;

pub mod manifest;

pub use crate::manifest::{Manifest, Version};
use chrono::{naive::NaiveDate, Duration, Local};
use std::{env, fs::File, io::Read, ops::Sub, path::PathBuf};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
struct Component {
    name: String,
    required: bool,
    version: Option<Version>,
}

impl Component {
    fn from(manifest: &Manifest, name: &str) -> Self {
        let required = match name {
            "rustc" | "cargo" => true,
            _ => false,
        };
        Component {
            name: name.to_string(),
            required,
            version: manifest.pkg_version(name),
        }
    }

    pub fn update_info(&self, other: Option<Version>) -> Option<String> {
        match (&self.version, &other) {
            (Some(version), Some(other)) => {
                if version < other {
                    Some(format!(
                        "{} - from {} to {}",
                        self.name,
                        version.to_string(),
                        other.to_string()
                    ))
                } else {
                    None
                }
            }
            _ => None,
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
        let (channel, target) = current_channel_target()?;
        let manifest = local_manifest()?;
        let components = installed_components(&target)?
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
        match self.manifest.pkg_version("rustc") {
            Some(version) => format!(
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
            ),
            None => String::from("Not found installed rustc"),
        }
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
    pub fn new() -> Option<Rust> {
        match Toolchain::new() {
            Ok(toolchain) => {
                let date = Local::today().naive_local();
                let manifest =
                    Manifest::from_date(&date.format("%Y-%m-%d").to_string(), &toolchain.channel)
                        .ok();
                Some(Rust {
                    offset: -1,
                    date,
                    toolchain,
                    manifest,
                })
            }
            Err(_) => None,
        }
    }

    pub fn from_date(date_str: &str) -> Option<Rust> {
        match Toolchain::new() {
            Ok(toolchain) => {
                let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
                let offset = (Local::today().naive_local() - date).num_days() - 1;
                let manifest = Manifest::from_date(date_str, &toolchain.channel).ok();
                Some(Rust {
                    offset,
                    date,
                    toolchain,
                    manifest,
                })
            }
            Err(_) => None,
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
                    match manifest.pkg_for_target(&component, &self.toolchain.target) {
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
            Some(manifest) => manifest.pkg_version(name),
            None => None,
        }
    }

    pub fn date_str(&self) -> String {
        self.date.format("%Y-%m-%d").to_string()
    }

    pub fn print_info(&self) {
        println!("{}", &self.toolchain.info());
    }

    fn update_info(&self) -> Option<Vec<String>> {
        if self.missing_components().is_empty() {
            let manifest = self.manifest.clone()?;
            Some(
                self.toolchain
                    .components
                    .iter()
                    .filter_map(|c| c.update_info(manifest.pkg_version(&c.name)))
                    .collect(),
            )
        } else {
            None
        }
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
        )
        .ok();
        Some(self.clone())
    }
}

fn current_channel_target() -> Result<(String, String), String> {
    let toolchain = env::var("RUSTUP_TOOLCHAIN").map_err(|e| e.to_string())?;
    let split: Vec<&str> = toolchain.splitn(2, '-').collect();
    let channel = split[0].to_string();
    let target = split[1].to_string();
    Ok((channel, target))
}

fn installed_components(target: &str) -> Result<Vec<String>, String> {
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

fn local_manifest() -> Result<Manifest, String> {
    let rustup_home = env::var("RUSTUP_HOME").map_err(|e| e.to_string())?;
    let toolchain = env::var("RUSTUP_TOOLCHAIN").map_err(|e| e.to_string())?;
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
    toml::from_str(&contents).map_err(|e| e.to_string())
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
    let rust = Rust::new().unwrap();
    rust.print_info();

    let v = rust
        .filter(|r| r.manifest.is_some() && r.missing_components().is_empty())
        .nth(0)
        .unwrap();

    match (
        v.offset,
        v.toolchain.manifest.pkg_version("rust") < v.manifest_pkg_version("rust"),
    ) {
        (0, true) => println!(
            "{}\nUse: \"rustup update\" (new version from {})",
            v.update_info().unwrap().iter().fold(
                String::from("Update components:\n"),
                |mut acc, c| {
                    acc.push_str(c);
                    acc.push('\n');
                    acc
                }
            ),
            v.date_str()
        ),
        (0, false) => println!("Current version is up to date"),
        _ => println!(
            "{}\nUse: \"rustup default {}-{}\"{}",
            v.update_info().unwrap().iter().fold(
                String::from("Update components:\n"),
                |mut acc, c| {
                    acc.push_str(c);
                    acc.push('\n');
                    acc
                }
            ),
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
