//! `rvsum anonymise` round trips on synthetic workbooks only.

use std::{path::Path, process::Command};

use calamine::{open_workbook, Data, Reader, Xlsx};

const FIXTURE: &str = "testdata/regression_anonymise.xlsx";

/// Invented identifiers in the fixture that must never appear in the output.
const SECRETS: &[&str] = &[
    "zulu",
    "alpha-payroll",
    "payroll",
    "kube-node-secret",
    "legacy-hr",
    "acme",
    "corp",
    "10.66.0",
    "jane doe",
    "team rocket",
    "dubai",
    "gold-datastore",
    "silver-datastore",
    "esx-acme",
    "vcenter",
    "6000c29",
    "u-zulu",
    "u-alpha",
    "u-hr",
    "u-kube",
    "naa.600",
    "secret",
    "licence-key",
    "owner-team",
    "-sort-",
];

fn rvsum(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("run binary");
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn anonymise(input: &str, out: &Path, extra: &[&str]) -> String {
    let mut a = vec![
        "anonymise",
        "-i",
        input,
        "-o",
        out.to_str().unwrap(),
        "--salt",
        "s1",
    ];
    a.extend(extra);
    let (ok, _, err) = rvsum(&a);
    assert!(ok, "anonymise failed: {err}");
    err
}

type Sheets = Vec<(String, Vec<Vec<Data>>)>;

fn read(path: &Path) -> Sheets {
    let mut wb: Xlsx<_> = open_workbook(path).unwrap();
    wb.sheet_names()
        .into_iter()
        .map(|n| {
            let r = wb.worksheet_range(&n).unwrap();
            let rows = r.rows().map(|r| r.to_vec()).collect();
            (n, rows)
        })
        .collect()
}

fn col(sheets: &Sheets, sheet: &str, header: &str) -> Vec<Data> {
    let rows = &sheets.iter().find(|s| s.0 == sheet).unwrap().1;
    let i = rows[0]
        .iter()
        .position(|h| matches!(h, Data::String(s) if s == header))
        .unwrap_or_else(|| panic!("{sheet}/{header} missing"));
    rows[1..].iter().map(|r| r[i].clone()).collect()
}

/// Summary rows (without names), totals and the count lines on stderr.
fn sizing(file: &str, flags: &[&str]) -> (String, String) {
    let mut a = vec!["-r", file, "--no-cache", "-s", "--format", "json"];
    a.extend(flags);
    let (ok, out, err) = rvsum(&a);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let mut rows: Vec<String> = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let mut r = r.clone();
            let o = r.as_object_mut().unwrap();
            o.remove("datacenter");
            o.remove("cluster");
            r.to_string()
        })
        .collect();
    rows.sort();
    let counts: Vec<&str> = err
        .lines()
        .filter(|l| !l.starts_with(' ') && !l.contains(".xlsx"))
        .collect();
    (format!("{rows:?} {}", v["totals"]), counts.join("\n"))
}

#[test]
fn no_secret_survives_and_values_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("anon.xlsx");
    let log = anonymise(FIXTURE, &out, &[]);
    assert!(log.contains("custom attributes renamed"), "{log}");

    let before = read(Path::new(FIXTURE));
    let after = read(&out);
    // Same sheets in the same order, same shape.
    assert_eq!(
        before.iter().map(|s| &s.0).collect::<Vec<_>>(),
        after.iter().map(|s| &s.0).collect::<Vec<_>>()
    );
    for ((name, a), (_, b)) in before.iter().zip(&after) {
        assert_eq!(a.len(), b.len(), "{name} rows");
        for (ra, rb) in a.iter().zip(b) {
            for (ca, cb) in ra.iter().zip(rb) {
                // Numbers, booleans and dates keep their type and value
                // (identifier columns such as Disk Key stay numbers too).
                match ca {
                    Data::Float(_) | Data::Int(_) | Data::Bool(_) | Data::DateTime(_)
                        if name != "vLicense" =>
                    {
                        assert_eq!(ca, cb, "{name}")
                    }
                    _ => {}
                }
                let text = cb.to_string().to_ascii_lowercase();
                for s in SECRETS {
                    assert!(!text.contains(s), "{name}: {s:?} leaked in {text:?}");
                }
            }
        }
    }
    // Custom attribute header renamed; kept values intact.
    assert!(col(&after, "vInfo", "Attr_1")
        .iter()
        .all(|v| v.to_string().starts_with("anon-")));
    assert_eq!(col(&after, "vInfo", "Tools")[0].to_string(), "toolsOk");
    assert_eq!(col(&after, "vDisk", "Disk")[0].to_string(), "Hard disk 1");
    assert_eq!(
        col(&after, "vDisk", "Sharing mode")[4].to_string(),
        "sharingMultiWriter"
    );
    assert_eq!(
        col(&after, "vMetaData", "RVTools version")[0].to_string(),
        "4.7.1"
    );
    assert_eq!(col(&after, "vLicense", "Key")[0], Data::Empty);
    let mounts: Vec<String> = col(&after, "vPartition", "Disk")
        .iter()
        .map(|d| d.to_string())
        .collect();
    assert_eq!(mounts[0], "C:\\");
    assert_eq!(mounts[2], "/");
    assert!(mounts[5].starts_with("/var/lib/kubelet/pods/anon-"));

    // The same name gets the same token on every sheet and inside paths.
    let vm_info = col(&after, "vInfo", "VM");
    let vm_disk = col(&after, "vDisk", "VM");
    assert_eq!(vm_info[0], vm_disk[0]);
    let ds = col(&after, "vDatastore", "Name")[0].to_string();
    let vm0 = vm_info[0].to_string();
    let path = col(&after, "vDisk", "Disk Path")[0].to_string();
    assert_eq!(path, format!("[{ds}] {vm0}/{vm0}.vmdk"));
    assert!(col(&after, "vCD", "Device Type")[0]
        .to_string()
        .starts_with(&format!("ISO [{ds}] ")));
    // VM tokens sort like the originals ("alpha" < "kube" < "Legacy" < "zulu" bytewise: L < a).
    let mut orig: Vec<String> = col(&before, "vInfo", "VM")
        .iter()
        .map(|d| d.to_string())
        .collect();
    let mut toks: Vec<(String, String)> = orig
        .iter()
        .cloned()
        .zip(vm_info.iter().map(|d| d.to_string()))
        .collect();
    orig.sort();
    toks.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(orig, toks.into_iter().map(|t| t.0).collect::<Vec<_>>());
}

#[test]
fn sizing_is_identical_after_anonymising() {
    let dir = tempfile::tempdir().unwrap();
    let mut files: Vec<String> =
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata"))
            .unwrap()
            .filter_map(|e| {
                let p = e.unwrap().path();
                let n = p.file_name()?.to_str()?.to_string();
                (n.ends_with(".xlsx") && n.starts_with("regression_"))
                    .then(|| format!("testdata/{n}"))
            })
            .collect();
    files.sort();
    assert!(files.len() > 5);
    for f in &files {
        let out = dir.path().join(Path::new(f).file_name().unwrap());
        anonymise(f, &out, &[]);
        let o = out.to_str().unwrap();
        for flags in [
            &[][..],
            &["--keep-rdm", "--keep-independent"],
            &["-i"],
            &["-d"],
        ] {
            assert_eq!(sizing(f, flags), sizing(o, flags), "{f} {flags:?}");
        }
    }
}

#[test]
fn salt_is_deterministic_and_mapping_is_private() {
    let dir = tempfile::tempdir().unwrap();
    let map = dir.path().join("map.csv");
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let files = format!("{FIXTURE},testdata/regression_coverage.xlsx");
    anonymise(&files, &a, &["--mapping-file", map.to_str().unwrap()]);
    let log = anonymise(&files, &b, &[]);
    assert!(!log.contains("random salt"));
    let a1 = read(&a.join("regression_anonymise_anon.xlsx"));
    let b1 = read(&b.join("regression_anonymise_anon.xlsx"));
    assert_eq!(a1, b1, "same salt, same tokens");
    assert!(b.join("regression_coverage_anon.xlsx").exists());

    let csv = std::fs::read_to_string(&map).unwrap();
    assert!(csv.starts_with("kind,original,token"));
    assert!(csv.contains("header,Owner-Team-Acme,Attr_1"));
    assert!(csv.contains("value,gold-datastore-acme,anon-"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&map).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // A different salt changes the hashed tokens.
    let c = dir.path().join("c.xlsx");
    let (ok, _, _) = rvsum(&[
        "anonymise",
        "-i",
        FIXTURE,
        "-o",
        c.to_str().unwrap(),
        "--salt",
        "other",
    ]);
    assert!(ok);
    assert_ne!(
        col(&read(&c), "vDatastore", "Name"),
        col(&a1, "vDatastore", "Name")
    );
}

#[test]
fn random_salt_warns_and_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.xlsx");
    let o = out.to_str().unwrap();
    let (ok, _, err) = rvsum(&["anonymise", "-i", FIXTURE, "-o", o]);
    assert!(ok && err.contains("random salt"), "{err}");
    let (ok, _, err) = rvsum(&["anonymise", "-i", FIXTURE, "-o", o]);
    assert!(!ok && err.contains("already exists"), "{err}");
    let (ok, _, _) = rvsum(&["anonymise", "-i", FIXTURE, "-o", o, "--force"]);
    assert!(ok);
    let (ok, _, err) = rvsum(&["anonymise", "-i", FIXTURE, "-o", FIXTURE, "--force"]);
    assert!(!ok && err.contains("overwrite its input"), "{err}");
}

#[test]
fn keep_column_overrides_hashing() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("k.xlsx");
    anonymise(FIXTURE, &out, &["--keep-column", "Datacenter"]);
    assert_eq!(
        col(&read(&out), "vInfo", "Datacenter")[0].to_string(),
        "DC-Acme-Dubai"
    );
}

#[test]
fn top_level_usage_is_unchanged() {
    let (ok, out, err) = rvsum(&[
        "-r",
        "testdata/regression_baseline.xlsx",
        "-s",
        "--format",
        "json",
    ]);
    assert!(ok, "{err}");
    assert!(out.contains("totals"));
}
