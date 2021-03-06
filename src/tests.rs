use super::*;
use crate::manifest::*;
use std::str::FromStr;

#[test]
fn test_component() {
    let comp = Component {
        name: String::from("test"),
        required: false,
        version: Version::from_str("1.31.6 (ae0d89a08 2019-01-12)").ok(),
    };
    let update = comp.update_info(Version::from_str("1.31.6 (000000000 2019-01-13)").ok());
    assert_eq!(
        update,
        Some(
            "test - from 1.31.6 (ae0d89a08 2019-01-12) to 1.31.6 (000000000 2019-01-13)"
                .to_string()
        )
    )
}

#[test]
fn test_version() {
    assert!(Version::from_str("rls-preview 1.31 (ae0d89a08 2019-01-13)").is_err());
    assert!(Version::from_str("1.31.21 (ae0d89a08 2019-01-13)").is_ok());
    let ver1 = Version::from_str("1.31.6 (ae0d89a08 2019-01-13)");
    let ver2 = Version::from_str("1.31.5 (ae0d89a08 2019-01-13)");
    let ver3 = Version::from_str("1.31.6 (ae0d89a08 2019-01-12)");
    let ver4 = Version::from_str("1.31.6 (000000000 2019-01-13)");
    assert!(ver1 > ver2);
    assert!(ver1 > ver3);
    assert!(ver1 == ver4);
    assert!(ver3 > ver2);
}

#[test]
fn test_printvec() {
    let test_vec = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    assert_eq!(print_vec(&test_vec, ""), "abc");
    assert_eq!(print_vec(&test_vec, ","), "a,b,c");
    assert_eq!(print_vec(&test_vec, " , "), "a , b , c");
}

#[test]
fn test_channel() {
    assert!(Channel::Beta > Channel::Stable);
    assert!(Channel::Nightly > Channel::Beta);
    assert!(Channel::Stable == Channel::Stable);
    assert!(Channel::Stable < Channel::Nightly);
}

#[test]
fn test_commit() {
    let c1 = Commit::from_str("12fa34b 2018-31-12");
    let c2 = Commit::from_str("(12fa34b 2018-12-31)");
    let c3 = Commit::from_str("12fa34a 2018-12-31");
    let c4 = Commit::from_str("12fa34b 2019-01-01");
    assert!(c1.is_err());
    assert!(c2.is_ok());
    assert!(c2 == c3);
    assert!(c3 < c4);
}

#[test]
fn test_wrong_path() {
    let path = "/dist/01-01-2019/channel-rust-nightly.toml";
    let manifest = Manifest::from_url(path);
    assert!(manifest.is_err());
    let path = "static.rust-lang.org";
    let manifest = Manifest::from_url(path);
    assert!(manifest.is_err());
}

#[test]
fn test_new_year_manifest() {
    let manifest_from_date = Manifest::from_date("2019-01-01", "nightly");
    let path = "/dist/2019-01-01/channel-rust-nightly.toml";
    let optional_manifest = Manifest::from_url(path);
    assert!(optional_manifest.is_ok());
    let manifest = optional_manifest.unwrap();
    assert_eq!(manifest_from_date.unwrap(), manifest);
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
    assert_eq!(manifest.pkg_version("rust"), Some(rust1330));
    let rust_src = manifest.pkg.get("rust-src").unwrap();
    let target_info = rust_src.target.get("x86_64-pc-windows-gnu");
    assert!(target_info.is_none());
    let target_info = rust_src.target.get("*");
    assert!(target_info.is_some());
    let target_info = target_info.unwrap();
    assert_eq!(target_info.available, true);
    assert_eq!(
        manifest
            .pkg_for_target("rust-src", "x86_64-pc-windows-gnu")
            .unwrap()
            .available,
        true
    )
}

#[test]
fn test_parse_version() {
    let s = "1.33.0-nightly (9eac38634 2018-12-31)";
    let split: Vec<&str> = s.splitn(2, ' ').collect();
    assert_eq!(split.len(), 2);
    let (raw_version, commit) = (split[0], split[1]);
    assert_eq!(raw_version, "1.33.0-nightly");
    let split: Vec<&str> = raw_version.split('-').collect();
    let (version, channel) = if split.len() == 2 {
        (split[0], split[1])
    } else {
        (split[0], "")
    };
    assert_eq!(version, "1.33.0");
    assert_eq!(channel, "nightly");
    assert_eq!(commit, "(9eac38634 2018-12-31)");
    let commit = Commit::from_str(&commit).unwrap();
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
    assert_eq!(
        ver,
        Version {
            version: version.to_string(),
            channel,
            commit
        }
    );
}

#[test]
fn test_parse_active_toolchain() {
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
