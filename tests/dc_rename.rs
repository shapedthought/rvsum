//! `--dc-rename` behaviour against synthetic fixtures only.
//! All inventory names here are invented (DC-A, DC-B, prod-dc-a, …).

use std::{fs, process::Command};

use arrow_array::Array;
use tempfile::TempDir;

const BASELINE: &str = "testdata/regression_baseline.xlsx";

fn run(args: &[&str]) -> (bool, String, String) {
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

fn write_map(dir: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn csv(args: &[&str]) -> (bool, String, String) {
    let mut a = args.to_vec();
    a.extend(["-s", "--format", "csv"]);
    run(&a)
}

fn totals(args: &[&str]) -> (f64, u64) {
    let mut a = args.to_vec();
    a.extend(["-s", "--format", "json"]);
    let (ok, stdout, stderr) = run(&a);
    assert!(ok, "failed: {stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    (
        v["totals"]["capacity_tb"].as_f64().unwrap(),
        v["totals"]["vm_count"].as_u64().unwrap(),
    )
}

#[test]
fn exact_match_relabels_and_leaves_unlisted() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"DC-A": "Dubai"}"#);
    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("Dubai,CL2,2.00,1"), "{stdout}");
    assert!(stdout.contains("Dubai,CL1,1.00,2"), "{stdout}");
    assert!(stdout.contains("DC-B,CL3,0.75,1"), "{stdout}");
    assert!(!stdout.contains("DC-A,"), "{stdout}");
    assert_eq!(
        totals(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]),
        (3.75, 4)
    );
}

#[test]
fn case_insensitive_fallback_when_no_exact_key() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"dc-a": "Dubai"}"#);
    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("Dubai,CL1,1.00,2"), "{stdout}");
    assert!(!stdout.contains("DC-A,"), "{stdout}");
}

#[test]
fn exact_match_preferred_over_case_insensitive() {
    let dir = TempDir::new().unwrap();
    let map = write_map(
        &dir,
        "rename.json",
        r#"{"DC-A": "Dubai", "dc-a": "ShouldNotWin"}"#,
    );
    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("Dubai,"), "{stdout}");
    assert!(!stdout.contains("ShouldNotWin"), "{stdout}");
}

#[test]
fn two_dcs_mapped_to_same_name_merge_without_double_counting() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"DC-A": "UAE", "DC-B": "UAE"}"#);
    let path = map.to_str().unwrap();
    assert_eq!(totals(&["-r", BASELINE, "--dc-rename", path]), (3.75, 4));
    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", path]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("UAE,CL1,1.00,2"), "{stdout}");
    assert!(stdout.contains("UAE,CL2,2.00,1"), "{stdout}");
    assert!(stdout.contains("UAE,CL3,0.75,1"), "{stdout}");
    assert!(!stdout.contains("DC-A,"), "{stdout}");
    assert!(!stdout.contains("DC-B,"), "{stdout}");

    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", path, "--flatten-site"]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("UAE,UAE_cluster,3.75,4"), "{stdout}");
    let lines: Vec<_> = stdout.lines().filter(|l| l.starts_with("UAE,")).collect();
    assert_eq!(lines.len(), 1, "{stdout}");
}

#[test]
fn unmatched_key_warns_and_drops_nothing() {
    let dir = TempDir::new().unwrap();
    let map = write_map(
        &dir,
        "rename.json",
        r#"{"DC-A": "Dubai", "no-such-dc": "Nowhere"}"#,
    );
    let (ok, stdout, stderr) = csv(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]);
    assert!(ok, "{stderr}");
    assert!(
        stderr.contains(
            "warning: --dc-rename key \"no-such-dc\" matches no datacenter in the inputs"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("\"DC-A\""), "{stderr}");
    assert!(stdout.contains("Dubai,"), "{stdout}");
    assert!(stdout.contains("DC-B,"), "{stdout}");
    assert_eq!(
        totals(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]),
        (3.75, 4)
    );
}

#[test]
fn flatten_site_uses_renamed_names() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"DC-A": "Dubai"}"#);
    let (ok, stdout, _) = csv(&[
        "-r",
        BASELINE,
        "--dc-rename",
        map.to_str().unwrap(),
        "--flatten-site",
    ]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("Dubai,Dubai_cluster,3.00,3"), "{stdout}");
    assert!(stdout.contains("DC-B,DC-B_cluster,0.75,1"), "{stdout}");
}

#[test]
fn anonymize_keeps_renamed_names_unhashed() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"DC-A": "Dubai"}"#);
    let (ok, stdout, _) = csv(&[
        "-r",
        BASELINE,
        "--dc-rename",
        map.to_str().unwrap(),
        "--anonymize",
    ]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("Dubai,"), "{stdout}");
    assert!(!stdout.contains("DC-A,"), "{stdout}");
    assert!(!stdout.contains("DC-B,"), "{stdout}");
    let hashed_b = fastmurmur3::hash(b"DC-B").to_string();
    assert!(stdout.contains(&hashed_b), "{stdout}");
    assert_eq!(
        totals(&[
            "-r",
            BASELINE,
            "--dc-rename",
            map.to_str().unwrap(),
            "--anonymize"
        ]),
        (3.75, 4)
    );
}

#[test]
fn filters_use_real_names_before_rename() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"DC-A": "Dubai"}"#);
    let path = map.to_str().unwrap();
    assert_eq!(
        totals(&["-r", BASELINE, "--dc-include", "DC-A", "--dc-rename", path]),
        (3.00, 3)
    );
    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-include", "DC-A", "--dc-rename", path]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("Dubai,"), "{stdout}");
    assert!(!stdout.contains("DC-B,"), "{stdout}");

    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-include", "Dubai", "--dc-rename", path]);
    assert!(ok, "{stdout}");
    assert!(
        stdout
            .lines()
            .all(|l| l.starts_with("datacenter,") || l.is_empty()),
        "filters must use real names, not display names: {stdout}"
    );
}

#[test]
fn works_with_html_action_and_parquet() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"DC-A": "Dubai"}"#);
    let html = dir.path().join("report.html");
    let actions = dir.path().join("actions.json");
    let parquet = dir.path().join("pq");
    let (ok, _, stderr) = run(&[
        "-r",
        BASELINE,
        "--dc-rename",
        map.to_str().unwrap(),
        "--html-report",
        html.to_str().unwrap(),
        "--action-report",
        actions.to_str().unwrap(),
        "--export-parquet",
        parquet.to_str().unwrap(),
        "--no-cache",
    ]);
    assert!(ok, "{stderr}");
    let html_body = fs::read_to_string(&html).unwrap();
    assert!(html_body.contains("Dubai"), "{html_body}");
    assert!(!html_body.contains(">DC-A<"), "{html_body}");
    assert!(html_body.contains("DC-B"), "{html_body}");

    if actions.exists() {
        let body = fs::read_to_string(&actions).unwrap();
        if body.trim() != "[]" {
            assert!(
                !body.contains("\"DC-A\""),
                "action report should use renamed DC: {body}"
            );
        }
    }

    let vms = parquet.join("vms.parquet");
    assert!(vms.exists(), "{stderr}");
    let file = fs::File::open(&vms).unwrap();
    let reader = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    let mut seen_dubai = false;
    let mut seen_a = false;
    let mut seen_b = false;
    for batch in reader {
        let batch = batch.unwrap();
        let col = batch
            .column_by_name("datacenter")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow_array::StringArray>()
            .unwrap();
        for i in 0..col.len() {
            match col.value(i) {
                "Dubai" => seen_dubai = true,
                "DC-A" => seen_a = true,
                "DC-B" => seen_b = true,
                _ => {}
            }
        }
    }
    assert!(seen_dubai, "parquet missing renamed DC");
    assert!(!seen_a, "parquet still has original DC-A");
    assert!(seen_b, "parquet dropped unlisted DC-B");
}

#[test]
fn missing_file_is_a_clear_error() {
    let (ok, _, stderr) = run(&[
        "-r",
        BASELINE,
        "--dc-rename",
        "/no/such/dc-rename.json",
        "-s",
    ]);
    assert!(!ok);
    assert!(stderr.contains("Datacenter rename error"), "{stderr}");
    assert!(stderr.contains("could not read"), "{stderr}");
}

#[test]
fn bad_json_is_a_clear_error() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "bad.json", "{not json");
    let (ok, _, stderr) = run(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap(), "-s"]);
    assert!(!ok);
    assert!(stderr.contains("Datacenter rename error"), "{stderr}");
    assert!(stderr.contains("invalid JSON"), "{stderr}");
}

#[test]
fn non_string_value_is_a_clear_error() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "bad.json", r#"{"DC-A": 12}"#);
    let (ok, _, stderr) = run(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap(), "-s"]);
    assert!(!ok);
    assert!(
        stderr.contains("must be a string or an array of strings"),
        "{stderr}"
    );
}

#[test]
fn empty_target_is_a_clear_error() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "bad.json", r#"{"DC-A": "  "}"#);
    let (ok, _, stderr) = run(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap(), "-s"]);
    assert!(!ok);
    assert!(stderr.contains("must not be empty"), "{stderr}");
}

#[test]
fn rejects_combination_with_site_map() {
    let dir = TempDir::new().unwrap();
    let rename = write_map(&dir, "rename.json", r#"{"DC-A": "Dubai"}"#);
    let site = write_map(
        &dir,
        "site.json",
        r#"[{"group_name":"Site1","dc_names":["DC-A"]}]"#,
    );
    let (ok, _, stderr) = run(&[
        "-r",
        BASELINE,
        "--dc-rename",
        rename.to_str().unwrap(),
        "--dc-site-map",
        site.to_str().unwrap(),
        "-s",
    ]);
    assert!(!ok);
    assert!(stderr.contains("--dc-rename"), "{stderr}");
    assert!(stderr.contains("--dc-site-map"), "{stderr}");
}

#[test]
fn array_form_merges_without_double_counting() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"UAE": ["DC-A", "DC-B"]}"#);
    let path = map.to_str().unwrap();
    assert_eq!(totals(&["-r", BASELINE, "--dc-rename", path]), (3.75, 4));
    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", path]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("UAE,CL1,1.00,2"), "{stdout}");
    assert!(stdout.contains("UAE,CL2,2.00,1"), "{stdout}");
    assert!(stdout.contains("UAE,CL3,0.75,1"), "{stdout}");
    assert!(!stdout.contains("DC-A,"), "{stdout}");
    assert!(!stdout.contains("DC-B,"), "{stdout}");

    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", path, "--flatten-site"]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("UAE,UAE_cluster,3.75,4"), "{stdout}");
}

#[test]
fn mixed_forms_in_one_file() {
    let dir = TempDir::new().unwrap();
    let map = write_map(
        &dir,
        "rename.json",
        r#"{"DC-A": "Dubai", "Abu Dhabi": ["DC-B"]}"#,
    );
    let (ok, stdout, _) = csv(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]);
    assert!(ok, "{stdout}");
    assert!(stdout.contains("Dubai,"), "{stdout}");
    assert!(stdout.contains("Abu Dhabi,"), "{stdout}");
    assert!(!stdout.contains("DC-A,"), "{stdout}");
    assert!(!stdout.contains("DC-B,"), "{stdout}");
    assert_eq!(
        totals(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]),
        (3.75, 4)
    );
}

#[test]
fn unmatched_array_item_warns() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "rename.json", r#"{"UAE": ["DC-A", "no-such-dc"]}"#);
    let (ok, stdout, stderr) = csv(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap()]);
    assert!(ok, "{stderr}");
    assert!(
        stderr.contains(
            "warning: --dc-rename key \"no-such-dc\" matches no datacenter in the inputs"
        ),
        "{stderr}"
    );
    assert!(stdout.contains("UAE,"), "{stdout}");
    assert!(stdout.contains("DC-B,"), "{stdout}");
}

#[test]
fn conflicting_assignments_are_a_clear_error() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "bad.json", r#"{"DC-A": "Dubai", "UAE": ["DC-A"]}"#);
    let (ok, _, stderr) = run(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap(), "-s"]);
    assert!(!ok);
    assert!(stderr.contains("Datacenter rename error"), "{stderr}");
    assert!(stderr.contains("DC-A"), "{stderr}");
    assert!(stderr.contains("Dubai"), "{stderr}");
    assert!(stderr.contains("UAE"), "{stderr}");
}

#[test]
fn empty_array_is_a_clear_error() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "bad.json", r#"{"UAE": []}"#);
    let (ok, _, stderr) = run(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap(), "-s"]);
    assert!(!ok);
    assert!(stderr.contains("must not be empty"), "{stderr}");
}

#[test]
fn non_string_array_item_is_a_clear_error() {
    let dir = TempDir::new().unwrap();
    let map = write_map(&dir, "bad.json", r#"{"UAE": ["DC-A", 12]}"#);
    let (ok, _, stderr) = run(&["-r", BASELINE, "--dc-rename", map.to_str().unwrap(), "-s"]);
    assert!(!ok);
    assert!(stderr.contains("must contain only strings"), "{stderr}");
}
