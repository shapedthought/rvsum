//! `--dc-rename`: relabel datacenters after filters and before grouping.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
};

use crate::models::errors::MyError;
use crate::models::rvtools::Vinfo;

/// JSON object map of real datacenter names to display names.
#[derive(Debug, Clone)]
pub struct DcRename {
    /// Trimmed key -> trimmed display name (exact match).
    exact: BTreeMap<String, String>,
    /// ASCII-lowercased trimmed key -> trimmed display name (fallback).
    casefold: BTreeMap<String, String>,
    /// Trimmed keys in file order, used for unmatched-key warnings.
    keys: Vec<String>,
    /// Display names that must stay unhashed under `--anonymize`.
    pub targets: HashSet<String>,
}

impl DcRename {
    pub fn load(path: &Path) -> Result<Self, MyError> {
        let text = fs::read_to_string(path).map_err(|e| {
            MyError::DcRename(format!(
                "could not read --dc-rename file {}: {e}",
                path.display()
            ))
        })?;
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            MyError::DcRename(format!(
                "invalid JSON in --dc-rename file {}: {e}",
                path.display()
            ))
        })?;
        let obj = value.as_object().ok_or_else(|| {
            MyError::DcRename(
                "--dc-rename file must be a JSON object mapping datacenter names to display names"
                    .into(),
            )
        })?;

        let mut exact = BTreeMap::new();
        let mut casefold = BTreeMap::new();
        let mut keys = Vec::new();
        let mut targets = HashSet::new();

        for (raw_key, raw_value) in obj {
            let key = raw_key.trim();
            if key.is_empty() {
                return Err(MyError::DcRename(
                    "--dc-rename map keys must not be empty".into(),
                ));
            }
            let Some(raw_target) = raw_value.as_str() else {
                return Err(MyError::DcRename(format!(
                    "--dc-rename value for key {key:?} must be a string"
                )));
            };
            let target = raw_target.trim();
            if target.is_empty() {
                return Err(MyError::DcRename(format!(
                    "--dc-rename display name for key {key:?} must not be empty"
                )));
            }
            let key = key.to_string();
            let target = target.to_string();
            exact.insert(key.clone(), target.clone());
            casefold
                .entry(key.to_ascii_lowercase())
                .or_insert_with(|| target.clone());
            keys.push(key);
            targets.insert(target);
        }

        Ok(Self {
            exact,
            casefold,
            keys,
            targets,
        })
    }

    /// Resolve a workbook datacenter name to its display name, if mapped.
    pub fn mapped_name(&self, name: &str) -> Option<&str> {
        let trimmed = name.trim();
        if let Some(target) = self.exact.get(trimmed) {
            return Some(target.as_str());
        }
        self.casefold
            .get(&trimmed.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn apply(&self, vms: &mut [Vinfo]) {
        for vm in vms {
            if let Some(target) = self.mapped_name(&vm.datacenter) {
                vm.datacenter = target.to_string();
            }
        }
    }

    /// Keys that would not rename any of the given (pre-rename) datacenter names.
    pub fn unmatched_keys<'a>(&self, datacenters: impl Iterator<Item = &'a str>) -> Vec<String> {
        let names: Vec<&str> = datacenters.map(str::trim).collect();
        let mut unmatched: Vec<String> = self
            .keys
            .iter()
            .filter(|key| !names.iter().any(|dc| self.key_matches_dc(key, dc)))
            .cloned()
            .collect();
        unmatched.sort();
        unmatched
    }

    fn key_matches_dc(&self, key: &str, dc: &str) -> bool {
        if dc == key {
            return true;
        }
        // Case-insensitive fallback is only used when no exact key matches this DC.
        !self.exact.contains_key(dc) && dc.eq_ignore_ascii_case(key)
    }

    pub fn warn_unmatched<'a>(&self, datacenters: impl Iterator<Item = &'a str>) {
        for key in self.unmatched_keys(datacenters) {
            eprintln!("warning: --dc-rename key {key:?} matches no datacenter in the inputs");
        }
    }
}

/// Hash `name` when anonymizing, unless it is a `--dc-rename` display name.
pub(crate) fn hide_dc(name: &str, anonymize: bool, preserved: &HashSet<String>) -> String {
    if anonymize
        && !name.is_empty()
        && !preserved.contains(name)
        && !preserved.contains(name.trim())
    {
        crate::anon(name)
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_map(contents: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    fn sample() -> DcRename {
        let file = write_map(r#"{"prod-dc-a": "Dubai", "dr-dc": "Abu Dhabi"}"#);
        DcRename::load(file.path()).unwrap()
    }

    #[test]
    fn exact_match_trims_whitespace() {
        let map = sample();
        assert_eq!(map.mapped_name("prod-dc-a"), Some("Dubai"));
        assert_eq!(map.mapped_name("  prod-dc-a  "), Some("Dubai"));
        assert_eq!(map.mapped_name("dr-dc"), Some("Abu Dhabi"));
    }

    #[test]
    fn case_insensitive_fallback_when_no_exact_key() {
        let map = sample();
        assert_eq!(map.mapped_name("PROD-DC-A"), Some("Dubai"));
        assert_eq!(map.mapped_name("Dr-Dc"), Some("Abu Dhabi"));
    }

    #[test]
    fn exact_match_wins_over_case_insensitive_key() {
        let file = write_map(r#"{"DC-A": "Exact", "dc-a": "Folded"}"#);
        let map = DcRename::load(file.path()).unwrap();
        assert_eq!(map.mapped_name("DC-A"), Some("Exact"));
        assert_eq!(map.mapped_name("dc-a"), Some("Folded"));
        // No exact key for this spelling, so the first case-folded insert wins.
        assert_eq!(map.mapped_name("Dc-A"), Some("Exact"));
    }

    #[test]
    fn unlisted_names_are_unchanged() {
        let map = sample();
        assert_eq!(map.mapped_name("lab-dc"), None);
        assert_eq!(map.mapped_name(""), None);
    }

    #[test]
    fn unmatched_keys_use_the_same_rules() {
        let map = sample();
        assert!(map
            .unmatched_keys(["prod-dc-a", "other"].into_iter())
            .contains(&"dr-dc".to_string()));
        assert!(map
            .unmatched_keys(["PROD-DC-A", "DR-DC"].into_iter())
            .is_empty());
        assert_eq!(
            map.unmatched_keys(["lab-dc"].into_iter()),
            vec!["dr-dc".to_string(), "prod-dc-a".to_string()]
        );
    }

    #[test]
    fn apply_renames_matching_vms_only() {
        let map = sample();
        let mut vms = vec![
            vinfo("prod-dc-a", "CL1"),
            vinfo("lab-dc", "CL2"),
            vinfo("DR-DC", "CL3"),
        ];
        map.apply(&mut vms);
        assert_eq!(vms[0].datacenter, "Dubai");
        assert_eq!(vms[0].cluster, "CL1");
        assert_eq!(vms[1].datacenter, "lab-dc");
        assert_eq!(vms[1].cluster, "CL2");
        assert_eq!(vms[2].datacenter, "Abu Dhabi");
    }

    #[test]
    fn missing_file_is_an_error() {
        let err = DcRename::load(Path::new("/no/such/dc-rename.json")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("could not read"), "{msg}");
        assert!(msg.contains("--dc-rename"), "{msg}");
    }

    #[test]
    fn bad_json_is_an_error() {
        let file = write_map("{not json");
        let msg = DcRename::load(file.path()).unwrap_err().to_string();
        assert!(msg.contains("invalid JSON"), "{msg}");
    }

    #[test]
    fn non_object_json_is_an_error() {
        let file = write_map(r#"["prod-dc-a", "Dubai"]"#);
        let msg = DcRename::load(file.path()).unwrap_err().to_string();
        assert!(msg.contains("JSON object"), "{msg}");
    }

    #[test]
    fn non_string_value_is_an_error() {
        let file = write_map(r#"{"prod-dc-a": 1}"#);
        let msg = DcRename::load(file.path()).unwrap_err().to_string();
        assert!(msg.contains("must be a string"), "{msg}");
        assert!(msg.contains("prod-dc-a"), "{msg}");
    }

    #[test]
    fn empty_target_is_an_error() {
        let file = write_map(r#"{"prod-dc-a": "   "}"#);
        let msg = DcRename::load(file.path()).unwrap_err().to_string();
        assert!(msg.contains("must not be empty"), "{msg}");
    }

    #[test]
    fn empty_key_is_an_error() {
        let file = write_map(r#"{"": "Dubai"}"#);
        let msg = DcRename::load(file.path()).unwrap_err().to_string();
        assert!(msg.contains("keys must not be empty"), "{msg}");
    }

    #[test]
    fn hide_dc_preserves_renamed_targets() {
        let preserved = HashSet::from(["Dubai".to_string()]);
        assert_eq!(hide_dc("Dubai", true, &preserved), "Dubai");
        assert_ne!(hide_dc("DC-B", true, &preserved), "DC-B");
        assert_eq!(hide_dc("DC-B", false, &preserved), "DC-B");
    }

    fn vinfo(dc: &str, cluster: &str) -> Vinfo {
        Vinfo {
            vcenter: String::new(),
            provisioned: None,
            vm_name: "vm".into(),
            datacenter: dc.into(),
            cluster: cluster.into(),
            capacity: 0.0,
            powerstate: "poweredOn".into(),
            host: String::new(),
            os: String::new(),
            vpartition_capacity: None,
            tools_status: None,
            vm_key: "0#2".into(),
            vm_uuid: String::new(),
            non_vadp_capacity: 0.0,
        }
    }
}
