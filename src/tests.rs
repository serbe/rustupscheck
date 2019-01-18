use super::*;

#[test]
fn printvec() {
    let test_vec = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    assert_eq!(print_vec(&test_vec, ""), "abc");
    assert_eq!(print_vec(&test_vec, ","), "a,b,c");
    assert_eq!(print_vec(&test_vec, " , "), "a , b , c");
}

#[test]
fn channel() {
    assert!(Channel::Beta > Channel::Stable);
    assert!(Channel::Nightly > Channel::Beta);
    assert!(Channel::Stable == Channel::Stable);
    assert!(Channel::Stable < Channel::Nightly);
}

#[test]
fn commit() {
    let c1 = Commit::from(("12fa34b", "2018-31-12"));
    let c2 = Commit::from(("12fa34b", "2018-12-31"));
    let c3 = Commit::from(("12fa34a", "2018-12-31"));
    let c4 = Commit::from(("12fa34b", "2019-01-01"));
    assert!(c1.is_err());
    assert!(c2.is_ok());
    assert!(c2 == c3);
    assert!(c3 < c4);
}

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
    assert_eq!(
        manifest
            .get_pkg_for_target("rust-src", "x86_64-pc-windows-gnu")
            .unwrap()
            .available,
        true
    )
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
