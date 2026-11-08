//! All inventory in this file is invented; no customer fixtures are used.
use rust_xlsxwriter::Workbook;
use std::{fs, path::Path, process::Command};

fn fixture(path: &Path) {
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet().set_name("vInfo").unwrap();
    let headers = [
        "VM",
        "Powerstate",
        "Datacenter",
        "Cluster",
        "In Use MiB",
        "Provisioned MiB",
        "VI SDK Server",
        "Tools",
        "Host",
        "OS",
    ];
    for (col, value) in headers.iter().enumerate() {
        sheet.write_string(0, col as u16, *value).unwrap();
    }
    for (row, name) in [
        (1, "<script>alert(\"vm&'\")</script>"),
        (2, "Example normal VM"),
    ] {
        for (col, value) in [
            (0, name),
            (1, "poweredOn"),
            (2, "DC<&>"),
            (3, "Cluster\"'"),
            (6, "VC<&>"),
            (7, "toolsOk"),
            (8, "host.example.invalid"),
            (9, "Example OS"),
        ] {
            sheet
                .write_string(
                    row,
                    col,
                    if row == 2 && col == 6 {
                        "VC Second"
                    } else {
                        value
                    },
                )
                .unwrap();
        }
        sheet.write_number(row, 4, 524288.0).unwrap();
        sheet.write_number(row, 5, 1048576.0).unwrap();
    }
    let sheet = wb.add_worksheet().set_name("vDisk").unwrap();
    for (col, value) in ["VM", "Powerstate", "Disk", "Capacity MiB", "Disk Mode"]
        .iter()
        .enumerate()
    {
        sheet.write_string(0, col as u16, *value).unwrap();
    }
    for (row, name, mode) in [
        (
            1,
            "<script>alert(\"vm&'\")</script>",
            "independentPersistent",
        ),
        (2, "Example normal VM", "persistent"),
    ] {
        sheet.write_string(row, 0, name).unwrap();
        sheet.write_string(row, 1, "poweredOn").unwrap();
        sheet.write_string(row, 2, "Disk <one>").unwrap();
        sheet.write_number(row, 3, 1048576.0).unwrap();
        sheet.write_string(row, 4, mode).unwrap();
    }
    wb.save(path).unwrap();
}

fn run(input: &Path, output: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .arg("-r")
        .arg(input)
        .arg("--html-report")
        .arg(output)
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn offline_report_sections_coverage_and_escaping() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("invented.xlsx");
    fixture(&input);
    let report = dir.path().join("report.html");
    let result = run(&input, &report, &["--no-cache"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let html = fs::read_to_string(&report).unwrap();
    for text in [
        "<!doctype html>",
        "At a glance",
        "Coverage split",
        "Capacity by cluster",
        "Capacity by vCenter",
        "Customer actions",
        "Not protected by image-level backup",
        "VC&lt;&amp;&gt;",
        "VC Second",
        "DC&lt;&amp;&gt;",
        "Cluster&quot;&#39;",
        "&lt;script&gt;alert(&quot;vm&amp;&#39;&quot;)&lt;/script&gt;",
        "Excluded: 1.00 TB",
        "Image-level: 0.50 TB",
        "Agent in estimate: 0.00 TB",
        "<strong>2</strong>",
        "<strong>2.00</strong>",
        "<strong>1.00</strong>",
        "aria-sort=none",
        "id=filter",
        "id=category",
    ] {
        assert!(html.contains(text), "missing {text}");
    }
    assert!(!html.contains("<script>alert"));
    assert!(!html.contains("<script src"));
    assert!(!html.contains("https://"));
    assert!(!html.contains("http://"));
    assert!(!html.contains("innerHTML"));
    // Refresh and cache hits preserve newly projected provisioning/placement data.
    assert!(run(&input, &report, &["--refresh-cache"]).status.success());
    assert_eq!(html, fs::read_to_string(&report).unwrap());
    assert!(run(&input, &report, &[]).status.success());
    assert_eq!(html, fs::read_to_string(&report).unwrap());
    assert!(run(&input, &report, &["--keep-independent"])
        .status
        .success());
    let kept = fs::read_to_string(&report).unwrap();
    assert!(kept.contains("Agent in estimate: 0.50 TB"));
    assert!(kept.contains("Excluded: 0.00 TB"));
}

#[test]
fn anonymize_hides_every_inventory_label_and_source_filename() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("invented.xlsx");
    fixture(&input);
    let report = dir.path().join("report.html");
    assert!(run(&input, &report, &["--anonymize"]).status.success());
    let html = fs::read_to_string(report).unwrap();
    for secret in [
        "VC&lt;&amp;&gt;",
        "VC Second",
        "DC&lt;&amp;&gt;",
        "Cluster&quot;&#39;",
        "alert(",
        "Example normal VM",
        "host.example.invalid",
        "Disk &lt;one&gt;",
        "invented.xlsx",
    ] {
        assert!(!html.contains(secret), "leaked {secret}");
    }
    assert!(html.contains("Excluded: 1.00 TB"));
}

#[test]
fn missing_metadata_and_empty_selection_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("report.html");
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/regression_baseline.xlsx");
    assert!(run(&input, &output, &[]).status.success());
    let html = fs::read_to_string(&output).unwrap();
    assert!(html.contains("Unknown vCenter"));
    assert!(html.contains("Provisioned capacity unavailable"));
    assert!(run(&input, &output, &["--dc-include", "does-not-exist"])
        .status
        .success());
    let html = fs::read_to_string(&output).unwrap();
    assert!(html.contains("No capacity to chart."));
    assert!(html.contains("No customer actions found"));
}

#[test]
fn action_report_and_html_reuse_identical_rows() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("report.html");
    let actions = dir.path().join("actions.json");
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/regression_coverage.xlsx");
    let result = Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .arg("-r")
        .arg(input)
        .arg("--html-report")
        .arg(&output)
        .arg("--action-report")
        .arg(&actions)
        .output()
        .unwrap();
    assert!(result.status.success());
    let rows: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(actions).unwrap()).unwrap();
    let html = fs::read_to_string(output).unwrap();
    assert!(html.contains(&format!("{} actions</p>", rows.as_array().unwrap().len())));
    for row in rows.as_array().unwrap() {
        for value in row.as_object().unwrap().values() {
            if let Some(text) = value.as_str() {
                let escaped = text
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&#39;");
                assert!(
                    html.contains(&format!("<td>{escaped}</td>")),
                    "missing action field {text}"
                );
            }
        }
    }
}

#[test]
fn io_errors_are_reported() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/regression_coverage.xlsx");
    let dir = tempfile::tempdir().unwrap();
    let result = run(&input, &dir.path().join("missing-parent/report.html"), &[]);
    assert!(!result.status.success());
}
