use calamine::{Data, Range};

use crate::models::errors::MyError;

pub trait ColPosition {
    fn get_col_pos(&self, col_name: &str) -> Result<usize, MyError>;
}

impl ColPosition for Range<Data> {
    fn get_col_pos(&self, col_name: &str) -> Result<usize, MyError> {
        let header = self.rows().next().ok_or_else(|| {
            MyError::EmptySheet(format!(
                "no header row while looking for column: {col_name}"
            ))
        })?;
        let wanted = col_name.trim();

        header
            .iter()
            .position(|x| matches!(x, Data::String(s) if s.trim() == wanted))
            .or_else(|| {
                header.iter().position(
                    |x| matches!(x, Data::String(s) if s.trim().eq_ignore_ascii_case(wanted)),
                )
            })
            .ok_or_else(|| MyError::ColumnPosition(col_name.to_string()))
    }
}

pub trait GetString {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError>;
}

impl GetString for Data {
    fn get_string_value(&self, item: String, row: usize) -> Result<String, MyError> {
        match self {
            Data::String(t) => Ok(t.to_string()),
            Data::Empty => Ok("None".to_string()),
            _ => Err(MyError::EnumToString(format!(
                "{item} - row {row} - Datatype {self:?}"
            ))),
        }
    }
}

pub trait GetFloat {
    fn get_float_value(&self, item: String, row: usize) -> Result<f64, MyError>;
}

impl GetFloat for Data {
    fn get_float_value(&self, item: String, row: usize) -> Result<f64, MyError> {
        match self {
            Data::Float(t) => Ok(*t),
            Data::Int(t) => Ok(*t as f64),
            Data::Empty => Ok(0.0),
            _ => Err(MyError::EnumToFloat(format!(
                "{item} - row {row} - Datatype {self:?}"
            ))),
        }
    }
}

/// Best-effort string for optional columns (Raw, modes) that may be Bool/Float/String.
pub fn as_display_string(cell: &Data) -> String {
    match cell {
        Data::String(t) => t.clone(),
        Data::Bool(b) => b.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => f.to_string(),
        Data::Empty => String::new(),
        _ => String::new(),
    }
}

pub fn is_physical_rdm_mode(raw_comp_mode: &str) -> bool {
    raw_comp_mode.to_ascii_lowercase().contains("physicalmode")
}

pub fn is_independent_disk_mode(disk_mode: &str) -> bool {
    let m = disk_mode.to_ascii_lowercase().replace(['-', '_', ' '], "");
    m.contains("independent")
}

pub fn is_shared_disk_mode(sharing: &str) -> bool {
    let s = sharing.to_ascii_lowercase().replace(['-', '_', ' '], "");
    // VMware VirtualDiskSharing::sharingMultiWriter; RVTools "Sharing" / "Sharing mode"
    s.contains("multiwriter") || s == "sharingmultiwriter"
}

pub fn tools_unreliable(tools: Option<&str>) -> bool {
    tools
        .map(|t| t.to_ascii_lowercase())
        .is_some_and(|t| t.contains("toolsnotinstalled") || t.contains("toolsnotrunning"))
}

pub fn is_shared_bus_mode(shared_bus: &str) -> bool {
    let s = shared_bus.to_ascii_lowercase();
    s.contains("physicalsharing") || s.contains("virtualsharing")
}

/// Guest mount paths created by container runtimes, which repeat real disks.
pub fn is_container_mount(mount: &str) -> bool {
    let m = mount.trim().to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "/var/lib/kubelet",
        "/var/lib/containers",
        "/var/lib/docker",
        "/var/lib/containerd",
        "/var/lib/rancher",
        "/run",
        "/var/run",
        "/proc",
        "/sys",
        "/dev",
    ];
    m.contains("overlay")
        || PREFIXES
            .iter()
            .any(|p| m == *p || m.starts_with(&format!("{p}/")))
}

#[cfg(test)]
mod col_position_tests {
    use super::*;

    fn header(cols: &[&str]) -> Range<Data> {
        let mut r = Range::new((0, 0), (1, cols.len() as u32 - 1));
        for (i, c) in cols.iter().enumerate() {
            r.set_value((0, i as u32), Data::String(c.to_string()));
        }
        r
    }

    #[test]
    fn empty_range_errors() {
        let r: Range<Data> = Range::empty();
        assert!(matches!(r.get_col_pos("VM"), Err(MyError::EmptySheet(_))));
    }

    #[test]
    fn whitespace_header_found() {
        let r = header(&["Powerstate", "  VM ", "Host"]);
        assert_eq!(r.get_col_pos("VM").unwrap(), 1);
    }

    #[test]
    fn case_different_header_found() {
        let r = header(&["Powerstate", "Provisioned MiB"]);
        assert_eq!(r.get_col_pos("provisioned mib").unwrap(), 1);
    }

    #[test]
    fn exact_match_preferred_over_case_insensitive() {
        let r = header(&["vm", "VM"]);
        assert_eq!(r.get_col_pos("VM").unwrap(), 1);
    }

    #[test]
    fn missing_column_errors() {
        let r = header(&["Powerstate", "Host"]);
        assert!(matches!(r.get_col_pos("VM"), Err(MyError::ColumnPosition(c)) if c == "VM"));
    }
}
