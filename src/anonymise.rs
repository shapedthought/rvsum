//! `rvsum anonymise`: rewrite RVTools workbooks into shareable copies.
//!
//! Every text value that could identify a customer is replaced by a salted
//! HMAC-SHA-256 token (`anon-` + 16 hex digits). The same original always maps
//! to the same token within a run (across sheets and files), so joins, de-dup
//! and grouping behave exactly as before and rvsum sizes the copy identically.
//! Columns are classified by header: a known-safe list is kept, known
//! identifiers are hashed, and anything unrecognised (custom attributes) is
//! hashed with its header renamed `Attr_N`.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use calamine::{open_workbook, Data, Reader, Xlsx};
use rust_xlsxwriter::{Format, Workbook};
use sha2::{Digest, Sha256};

use crate::models::cli::AnonymiseArgs;

/// Standard RVTools headers whose values are safe to keep (lowercase).
const KEEP: &[&str] = &[
    "powerstate",
    "template",
    "srm placeholder",
    "config status",
    "connection state",
    "guest state",
    "heartbeat",
    "consolidation needed",
    "poweron",
    "suspended to memory",
    "suspend time",
    "suspend interval",
    "creation date",
    "change version",
    "cpus",
    "overall cpu readiness",
    "memory",
    "active memory",
    "nics",
    "disks",
    "fixed passthru hotplug",
    "min required evc mode key",
    "latency sensitivity",
    "op notification timeout",
    "enableuuid",
    "cbt",
    "num monitors",
    "das protection",
    "ft state",
    "ft role",
    "ft latency",
    "ft bandwidth",
    "ft sec. latency",
    "vm failover in progress",
    "ha restart priority",
    "ha isolation response",
    "ha vm monitoring",
    "boot required",
    "boot delay",
    "boot retry delay",
    "boot retry enabled",
    "boot bios setup",
    "reboot poweroff",
    "efi secure boot",
    "firmware",
    "hw version",
    "hw upgrade status",
    "hw upgrade policy",
    "hw target",
    "os according to the configuration file",
    "os according to the vmware tools",
    "os",
    "vi sdk server type",
    "vi sdk api version",
    "sockets",
    "cores p/s",
    "max",
    "overall",
    "level",
    "shares",
    "reservation",
    "entitlement",
    "drs entitlement",
    "limit",
    "hot add",
    "hot remove",
    "numa hotadd exposed",
    "memory reservation locked to max",
    "overhead",
    "consumed",
    "consumed overhead",
    "private",
    "shared",
    "swapped",
    "ballooned",
    "active",
    "disk key",
    "raw",
    "disk mode",
    "mode",
    "sharing mode",
    "sharing",
    "thin",
    "eagerly scrub",
    "split",
    "write through",
    "controller",
    "label",
    "scsi unit #",
    "unit #",
    "shared bus",
    "raw comp. mode",
    "nic label",
    "adapter",
    "connected",
    "starts connected",
    "type",
    "direct path io",
    "device node",
    "device type",
    "family",
    "speed",
    "ehci enabled",
    "auto connect",
    "bus number",
    "unit number",
    "date / time",
    "quiesced",
    "state",
    "vm version",
    "tools",
    "tools version",
    "required version",
    "upgradeable",
    "upgrade policy",
    "sync time",
    "app status",
    "heartbeat status",
    "kernel crash state",
    "operation ready",
    "state change support",
    "interactive guest",
    "os type",
    "api type",
    "api version",
    "version",
    "patch level",
    "build",
    "fullname",
    "product name",
    "product version",
    "product line",
    "vendor",
    "status",
    "overallstatus",
    "numhosts",
    "numeffectivehosts",
    "totalcpu",
    "numcpucores",
    "numcputhreads",
    "effective cpu",
    "totalmemory",
    "effective memory",
    "num vmotions",
    "ha enabled",
    "failover level",
    "admissioncontrolenabled",
    "host monitoring",
    "hb datastore candidate policy",
    "isolation response",
    "restart priority",
    "cluster settings",
    "max failures",
    "max failure window",
    "failure interval",
    "min up time",
    "vm monitoring",
    "drs enabled",
    "drs default vm behavior",
    "drs vmotion rate",
    "dpm enabled",
    "dpm default behavior",
    "dpm host power action rate",
    "compliance check state",
    "in maintenance mode",
    "in quarantine mode",
    "cpu model",
    "ht available",
    "ht active",
    "cores per cpu",
    "console",
    "vms per core",
    "vcpus per core",
    "vram",
    "vm used memory",
    "vm memory swapped",
    "vm memory ballooned",
    "vmotion support",
    "storage vmotion support",
    "current evc",
    "max evc",
    "ats heartbeat",
    "ats locking",
    "current cpu power man. policy",
    "supported cpu power man.",
    "host power policy",
    "esx version",
    "boot time",
    "dhcp",
    "ntpd running",
    "time zone",
    "time zone name",
    "gmt offset",
    "model",
    "bios vendor",
    "bios version",
    "bios date",
    "certificate start date",
    "certificate expiry date",
    "certificate status",
    "memory tiering type",
    "device",
    "bus",
    "pci",
    "driver",
    "network device",
    "duplex",
    "wakeon",
    "free ports",
    "promiscuous mode",
    "mac changes",
    "forged transmits",
    "traffic shaping",
    "width",
    "peak",
    "burst",
    "policy",
    "reverse policy",
    "notify switch",
    "rolling order",
    "offload",
    "tso",
    "zero copy xmit",
    "mtu",
    "vlan",
    "created",
    "max ports",
    "in traffic shaping",
    "in avg",
    "in peak",
    "in burst",
    "out traffic shaping",
    "out avg",
    "out peak",
    "out burst",
    "cdp type",
    "cdp operation",
    "lacp mode",
    "lacp load balance alg.",
    "max mtu",
    "full duplex",
    "blocked",
    "allow promiscuous",
    "check beacon",
    "live port moving",
    "check duplex",
    "check speed",
    "percentage",
    "block override",
    "config reset",
    "shaping override",
    "vendor config override",
    "sec. policy override",
    "teaming override",
    "vlan override",
    "accessible",
    "sioc enabled",
    "sioc threshold",
    "block size",
    "max blocks",
    "major version",
    "vmfs upgradeable",
    "mha",
    "oper. state",
    "vstorage",
    "queue depth",
    "revision",
    "cost unit",
    "total",
    "used",
    "expiration date",
    "features",
    "file type",
    "file size in bytes",
    "message type",
    "rvtools major version",
    "rvtools version",
    "xlsx creation datetime",
];

/// Standard RVTools headers holding identifiers: values hashed, header kept.
const HASH: &[&str] = &[
    "vm",
    "dns name",
    "primary ip address",
    "resource pool",
    "folder id",
    "folder",
    "vapp",
    "cluster rule(s)",
    "cluster rule name(s)",
    "annotation",
    "datacenter",
    "cluster",
    "host",
    "customization info",
    "guest detailed data",
    "vm id",
    "smbios uuid",
    "vm uuid",
    "vm instance uuid",
    "instance uuid",
    "bios uuid",
    "uuid",
    "vi sdk server",
    "vi sdk uuid",
    "disk uuid",
    "raw lun id",
    "internal sort column",
    "network",
    "switch",
    "mac address",
    "ipv4 address",
    "ipv6 address",
    "vmref",
    "name",
    "description",
    "resource pool name",
    "resource pool path",
    "object id",
    "vsan fault domain name",
    "assigned license(s)",
    "dns servers",
    "domain",
    "domain list",
    "dns search order",
    "ntp server(s)",
    "serial number",
    "service tag",
    "oem specific string",
    "certificate issuer",
    "certificate subject",
    "automdeploy.machineidentity",
    "autodeploy.machineidentity",
    "wwn",
    "mac",
    "uplink port",
    "port group",
    "ip address",
    "ip 6 address",
    "subnet mask",
    "gateway",
    "ip 6 gateway",
    "host members",
    "contact",
    "admin name",
    "lacp name",
    "port",
    "active uplink",
    "standby uplink",
    "address",
    "hosts",
    "cluster name",
    "url",
    "datastore",
    "display name",
    "serial #",
    "labels",
    "message",
    "server",
    "dvs",
    "vds",
];

/// Columns whose values are datastore/file paths: hashed segment by segment.
const PATHS: &[&str] = &[
    "path",
    "disk path",
    "log directory",
    "snapshot directory",
    "suspend directory",
    "filename",
    "file name",
    "friendly path name",
];

/// Columns blanked entirely.
const DROP: &[&str] = &["key"];

/// vPartition mount points kept as-is because they are generic.
const GENERIC_MOUNTS: &[&str] = &[
    "/",
    "/boot",
    "/boot/efi",
    "/home",
    "/var",
    "/var/log",
    "/tmp",
    "/opt",
    "/usr",
    "/srv",
];

/// Mount-path segments that are standard OS or container-runtime directory
/// names. They are kept so rvsum still recognises container mounts.
const GENERIC_SEGMENTS: &[&str] = &[
    "var",
    "lib",
    "kubelet",
    "containers",
    "docker",
    "containerd",
    "rancher",
    "run",
    "proc",
    "sys",
    "dev",
    "boot",
    "efi",
    "home",
    "tmp",
    "opt",
    "usr",
    "srv",
    "log",
    "mnt",
    "media",
    "etc",
    "root",
    "bin",
    "sbin",
    "local",
    "share",
    "snap",
    "pods",
    "volumes",
    "overlay",
    "overlay2",
    "merged",
    "k3s",
    "cache",
    "spool",
    "shm",
    "user",
    "windows",
    "program files",
    "users",
    "temp",
];

/// Placeholder values kept in every column (rvsum treats them as "no value").
const SENTINELS: &[&str] = &["none", "n/a", "-", "unknown", "null", "--"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Keep,
    Hash,
    Path,
    Mount,
    Label,
    Drop,
    Custom,
}

fn classify(sheet: &str, header: &str, keep_cols: &HashSet<String>) -> Class {
    let h = header.trim().to_ascii_lowercase();
    if keep_cols.contains(&h) {
        return Class::Keep;
    }
    if h == "disk" {
        return if sheet.eq_ignore_ascii_case("vpartition") {
            Class::Mount
        } else {
            Class::Label
        };
    }
    if DROP.contains(&h.as_str()) {
        return Class::Drop;
    }
    if PATHS.contains(&h.as_str()) {
        return Class::Path;
    }
    if HASH.contains(&h.as_str())
        || h.starts_with("network #")
        || (h.starts_with("path ") && !h.ends_with(" state"))
    {
        return Class::Hash;
    }
    if KEEP.contains(&h.as_str()) || is_measure(&h) {
        return Class::Keep;
    }
    Class::Custom
}

/// Standard measurement-style headers (units, counts, resource-pool stats).
fn is_measure(h: &str) -> bool {
    [" mb", " mib", " kib", " gb", " gib", " %"]
        .iter()
        .any(|u| h.ends_with(u) || h.contains(&format!("{u} (")))
        || h.starts_with("# ")
        || (h.starts_with("path ") && h.ends_with(" state"))
        || ["cpu ", "mem ", "qs "]
            .iter()
            .any(|p| h.starts_with(p) && !h.contains("name"))
}

struct Tokens {
    salt: Vec<u8>,
    map: BTreeMap<String, String>,
    /// VM names get tokens in their original sort order (see `rank_vm_names`).
    ranked: HashMap<String, String>,
}

impl Tokens {
    fn token(&mut self, original: &str) -> String {
        if let Some(t) = self.map.get(original) {
            return t.clone();
        }
        if let Some(t) = self.ranked.get(original) {
            self.map.insert(original.to_string(), t.clone());
            return t.clone();
        }
        let mac = hmac_sha256(&self.salt, original.as_bytes());
        let hex: String = mac[..8].iter().map(|b| format!("{b:02x}")).collect();
        let t = format!("anon-{hex}");
        self.map.insert(original.to_string(), t.clone());
        t
    }

    /// Token for a whole identifier value, or a datastore path when it looks like one.
    fn value(&mut self, raw: &str, uuid_like: bool) -> String {
        let s = raw.trim();
        if s.is_empty() || is_sentinel(s) {
            return raw.to_string();
        }
        if s.starts_with('[') && s.contains(']') {
            return self.path(s);
        }
        if uuid_like {
            self.token(&s.to_ascii_lowercase())
        } else {
            self.token(s)
        }
    }

    /// `[datastore] folder/file.vmdk` or a plain / or \ path, hashed per segment.
    fn path(&mut self, raw: &str) -> String {
        let s = raw.trim();
        if s.is_empty() || is_sentinel(s) {
            return raw.to_string();
        }
        let mut out = String::new();
        let mut rest = s;
        if let Some(stripped) = s.strip_prefix('[') {
            if let Some(end) = stripped.find(']') {
                out.push('[');
                out.push_str(&self.token(stripped[..end].trim()));
                out.push(']');
                rest = &stripped[end + 1..];
                if let Some(r) = rest.strip_prefix(' ') {
                    out.push(' ');
                    rest = r;
                }
            }
        }
        let mut seg = String::new();
        for ch in rest.chars() {
            if ch == '/' || ch == '\\' {
                out.push_str(&self.segment(&seg));
                out.push(ch);
                seg.clear();
            } else {
                seg.push(ch);
            }
        }
        out.push_str(&self.segment(&seg));
        out
    }

    fn segment(&mut self, seg: &str) -> String {
        if seg.is_empty() || is_drive(seg) {
            return seg.to_string();
        }
        if let Some((stem, ext)) = seg.rsplit_once('.') {
            if !stem.is_empty()
                && (1..=6).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
            {
                return format!("{}.{ext}", self.token(stem));
            }
        }
        self.token(seg)
    }

    /// Keep text as-is unless it embeds a `[datastore] path`, which is hashed.
    fn embedded_path(&mut self, raw: &str) -> String {
        match raw.find('[') {
            Some(i) if raw[i..].contains(']') => format!("{}{}", &raw[..i], self.path(&raw[i..])),
            _ => raw.to_string(),
        }
    }

    fn mount(&mut self, raw: &str) -> String {
        let s = raw.trim();
        let lower = s.to_ascii_lowercase();
        if GENERIC_MOUNTS.contains(&lower.trim_end_matches('/'))
            || GENERIC_MOUNTS.contains(&lower.as_str())
            || is_drive(s.trim_end_matches('\\'))
        {
            return raw.to_string();
        }
        let mut out = String::new();
        let mut seg = String::new();
        for ch in s.chars() {
            if ch == '/' || ch == '\\' {
                out.push_str(&self.mount_segment(&seg));
                out.push(ch);
                seg.clear();
            } else {
                seg.push(ch);
            }
        }
        out.push_str(&self.mount_segment(&seg));
        out
    }

    fn mount_segment(&mut self, seg: &str) -> String {
        let l = seg.to_ascii_lowercase();
        if seg.is_empty() || is_drive(seg) || GENERIC_SEGMENTS.contains(&l.as_str()) {
            return seg.to_string();
        }
        let t = self.token(seg);
        // rvsum ignores any mount containing "overlay"; keep that signal.
        if l.contains("overlay") {
            format!("{t}-overlay")
        } else {
            t
        }
    }
}

/// Identifier columns rvsum compares case-insensitively; tokens ignore case there too.
fn case_insensitive(header: &str) -> bool {
    let h = header.to_ascii_lowercase();
    ["uuid", "raw lun id", "vi sdk", "mac", "wwn"]
        .iter()
        .any(|p| h.contains(p))
}

fn is_sentinel(s: &str) -> bool {
    SENTINELS.iter().any(|x| s.eq_ignore_ascii_case(x))
}

fn is_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

fn is_disk_label(s: &str) -> bool {
    let l = s.trim().to_ascii_lowercase();
    ["hard disk ", "disk "].iter().any(|p| {
        l.strip_prefix(p)
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    })
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

#[derive(Default)]
struct Stats {
    kept: usize,
    hashed: usize,
    renamed: usize,
    dropped: usize,
}

pub fn run(args: &AnonymiseArgs) -> Result<()> {
    let salt = match (&args.salt, &args.salt_file) {
        (Some(s), _) => s.as_bytes().to_vec(),
        (None, Some(f)) => fs::read_to_string(f)
            .with_context(|| format!("reading salt file {}", f.display()))?
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .as_bytes()
            .to_vec(),
        (None, None) => {
            let mut b = [0u8; 32];
            getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("random salt: {e}"))?;
            eprintln!(
                "Note: using a random salt, so tokens differ on every run. Pass --salt or --salt-file to get the same tokens again."
            );
            b.to_vec()
        }
    };
    if salt.is_empty() {
        bail!("the salt must not be empty");
    }

    let outputs = plan_outputs(&args.input, &args.output)?;
    for (input, output) in &outputs {
        if same_file(input, output) {
            bail!("output {} would overwrite its input", output.display());
        }
        if output.exists() && !args.force {
            bail!(
                "{} already exists (use --force to overwrite)",
                output.display()
            );
        }
    }

    let keep_cols: HashSet<String> = args
        .keep_column
        .iter()
        .map(|c| c.trim().to_ascii_lowercase())
        .collect();
    let mut tokens = Tokens {
        salt,
        map: BTreeMap::new(),
        ranked: rank_vm_names(&outputs)?,
    };
    let mut attrs: HashMap<String, String> = HashMap::new();

    for (input, output) in &outputs {
        let stats = anonymise_file(input, output, &keep_cols, &mut tokens, &mut attrs)?;
        eprintln!(
            "Anonymised {} -> {}: {} sheet(s); columns kept {}, hashed {}, custom attributes renamed {}, dropped {}",
            input.display(),
            output.display(),
            stats.0,
            stats.1.kept,
            stats.1.hashed,
            stats.1.renamed,
            stats.1.dropped
        );
    }

    if let Some(path) = &args.mapping_file {
        write_mapping(path, &tokens.map, &attrs)?;
        eprintln!(
            "WARNING: {} maps tokens back to real names. It is as sensitive as the original export; never share it.",
            path.display()
        );
    }
    Ok(())
}

/// rvsum credits a disk shared by several VMs to the VM whose name sorts
/// first, so VM tokens must sort like the originals for identical sizing.
/// Tokens are `vm-` plus the zero-padded rank among all VM names in this run;
/// they reveal only the relative alphabetical order of VM names.
fn rank_vm_names(files: &[(PathBuf, PathBuf)]) -> Result<HashMap<String, String>> {
    let mut names = std::collections::BTreeSet::new();
    for (input, _) in files {
        let mut excel: Xlsx<_> =
            open_workbook(input).with_context(|| format!("opening {}", input.display()))?;
        for sheet in excel.sheet_names() {
            let Ok(range) = excel.worksheet_range(&sheet) else {
                continue;
            };
            let mut rows = range.rows();
            let Some(header) = rows.next() else { continue };
            let Some(col) = header
                .iter()
                .position(|h| matches!(h, Data::String(s) if s.trim().eq_ignore_ascii_case("vm")))
            else {
                continue;
            };
            for row in rows {
                let v = match row.get(col) {
                    Some(Data::String(s)) => s.trim().to_string(),
                    Some(Data::Int(i)) => i.to_string(),
                    Some(Data::Float(f)) => number_text(*f),
                    _ => continue,
                };
                if !v.is_empty() && !is_sentinel(&v) {
                    names.insert(v);
                }
            }
        }
    }
    let width = names.len().to_string().len().max(4);
    Ok(names
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n, format!("vm-{:0width$}", i + 1)))
        .collect())
}

fn plan_outputs(inputs: &[PathBuf], output: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let is_file = inputs.len() == 1
        && !output.is_dir()
        && output
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"));
    if is_file {
        return Ok(vec![(inputs[0].clone(), output.to_path_buf())]);
    }
    if output.exists() && !output.is_dir() {
        bail!("with several inputs, --output must be a folder");
    }
    fs::create_dir_all(output)?;
    let mut seen = HashSet::new();
    inputs
        .iter()
        .map(|i| {
            let stem = i.file_stem().and_then(|s| s.to_str()).unwrap_or("rvtools");
            let o = output.join(format!("{stem}_anon.xlsx"));
            if !seen.insert(o.clone()) {
                bail!("two inputs would both write {}", o.display());
            }
            Ok((i.clone(), o))
        })
        .collect()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn anonymise_file(
    input: &Path,
    output: &Path,
    keep_cols: &HashSet<String>,
    tokens: &mut Tokens,
    attrs: &mut HashMap<String, String>,
) -> Result<(usize, Stats)> {
    let mut excel: Xlsx<_> =
        open_workbook(input).with_context(|| format!("opening {}", input.display()))?;
    let mut wb = Workbook::new();
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
    let mut stats = Stats::default();
    let names = excel.sheet_names();
    for name in &names {
        let range = excel
            .worksheet_range(name)
            .with_context(|| format!("reading sheet {name}"))?;
        let ws = wb.add_worksheet_with_constant_memory()?;
        ws.set_name(name.as_str())?;
        let Some((r0, c0)) = range.start() else {
            continue;
        };
        let mut classes: Vec<Class> = Vec::new();
        let mut uuid_cols: Vec<bool> = Vec::new();
        for (ri, row) in range.rows().enumerate() {
            let r = r0 + ri as u32;
            if ri == 0 {
                for (ci, cell) in row.iter().enumerate() {
                    let c = (c0 + ci as u32) as u16;
                    let header = match cell {
                        Data::String(s) => s.clone(),
                        Data::Empty => String::new(),
                        other => other.to_string(),
                    };
                    let class = if header.trim().is_empty() {
                        Class::Hash
                    } else {
                        classify(name, &header, keep_cols)
                    };
                    match class {
                        Class::Keep | Class::Label => stats.kept += 1,
                        Class::Drop => stats.dropped += 1,
                        Class::Custom => stats.renamed += 1,
                        _ => stats.hashed += 1,
                    }
                    classes.push(class);
                    uuid_cols.push(case_insensitive(&header));
                    if header.is_empty() {
                        continue;
                    }
                    let out = if class == Class::Custom {
                        let n = attrs.len() + 1;
                        attrs
                            .entry(header.trim().to_string())
                            .or_insert_with(|| format!("Attr_{n}"))
                            .clone()
                    } else {
                        header
                    };
                    ws.write_string(r, c, out)?;
                }
                continue;
            }
            for (ci, cell) in row.iter().enumerate() {
                let c = (c0 + ci as u32) as u16;
                let class = classes.get(ci).copied().unwrap_or(Class::Custom);
                let header_uuid = uuid_cols.get(ci).copied().unwrap_or(false);
                write_cell(ws, r, c, cell, class, header_uuid, tokens, &date_fmt)?;
            }
        }
    }
    wb.save(output)
        .with_context(|| format!("writing {}", output.display()))?;
    Ok((names.len(), stats))
}

fn number_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

fn write_cell(
    ws: &mut rust_xlsxwriter::Worksheet,
    r: u32,
    c: u16,
    cell: &Data,
    class: Class,
    uuid_like: bool,
    tokens: &mut Tokens,
    date_fmt: &Format,
) -> Result<()> {
    let sensitive = !matches!(class, Class::Keep | Class::Label);
    match cell {
        Data::Empty | Data::Error(_) => {}
        _ if class == Class::Drop => {}
        Data::Bool(b) => {
            ws.write_boolean(r, c, *b)?;
        }
        Data::DateTime(d) => {
            ws.write_number_with_format(r, c, d.as_f64(), date_fmt)?;
        }
        Data::DateTimeIso(s) | Data::DurationIso(s) => {
            ws.write_string(r, c, s)?;
        }
        Data::Int(i) if sensitive => {
            ws.write_string(r, c, tokens.value(&i.to_string(), uuid_like))?;
        }
        Data::Float(f) if sensitive => {
            ws.write_string(r, c, tokens.value(&number_text(*f), uuid_like))?;
        }
        Data::Int(i) => {
            ws.write_number(r, c, *i as f64)?;
        }
        Data::Float(f) => {
            ws.write_number(r, c, *f)?;
        }
        Data::String(s) => {
            let out = match class {
                Class::Label
                    if is_disk_label(s) || s.trim().is_empty() || is_sentinel(s.trim()) =>
                {
                    s.clone()
                }
                // A kept column can still embed a datastore path, e.g. vCD
                // "ISO [datastore] folder/file.iso".
                Class::Keep => tokens.embedded_path(s),
                Class::Path => tokens.path(s),
                Class::Mount => tokens.mount(s),
                _ => tokens.value(s, uuid_like),
            };
            ws.write_string(r, c, out)?;
        }
    }
    Ok(())
}

fn write_mapping(
    path: &Path,
    map: &BTreeMap<String, String>,
    attrs: &HashMap<String, String>,
) -> Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    let mut w = csv::Writer::from_writer(file);
    w.write_record(["kind", "original", "token"])?;
    let mut headers: Vec<_> = attrs.iter().collect();
    headers.sort_by(|a, b| a.1.cmp(b.1));
    for (orig, tok) in headers {
        w.write_record(["header", orig, tok])?;
    }
    for (orig, tok) in map {
        w.write_record(["value", orig, tok])?;
    }
    w.flush()?;
    w.into_inner()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Tokens {
        Tokens {
            salt: b"s".to_vec(),
            map: BTreeMap::new(),
            ranked: HashMap::new(),
        }
    }

    #[test]
    fn hmac_matches_rfc4231_case2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn datastore_paths_share_tokens() {
        let mut k = t();
        let p = k.path("[ds-gold] app01/app01_1.vmdk");
        let ds = k.token("ds-gold");
        let vm = k.token("app01");
        assert!(p.starts_with(&format!("[{ds}] {vm}/")));
        assert!(p.ends_with(".vmdk"));
        assert!(!p.contains("gold") && !p.contains("app01"));
        assert_eq!(
            k.value("[ds-gold] app01/app01.vmx", false),
            k.path("[ds-gold] app01/app01.vmx")
        );
    }

    #[test]
    fn mounts_keep_only_generic() {
        let mut k = t();
        assert_eq!(k.mount("/"), "/");
        assert_eq!(k.mount("C:\\"), "C:\\");
        assert_eq!(k.mount("/boot"), "/boot");
        let m = k.mount("/u01/oradata/PAYROLL");
        assert!(!m.contains("PAYROLL") && m.starts_with('/'));
        let c = k.mount("/var/lib/kubelet/pods/p-secret/volumes");
        assert!(c.starts_with("/var/lib/kubelet/pods/") && !c.contains("secret"));
        assert!(crate::helpers::is_container_mount(&c));
        assert!(crate::helpers::is_container_mount(
            &k.mount("/data/myoverlayfs")
        ));
    }

    #[test]
    fn classification() {
        let none = HashSet::new();
        assert_eq!(classify("vInfo", "VM", &none), Class::Hash);
        assert_eq!(classify("vInfo", "In Use MiB", &none), Class::Keep);
        assert_eq!(classify("vInfo", "Capacity MB", &none), Class::Keep);
        assert_eq!(classify("vPartition", "Disk", &none), Class::Mount);
        assert_eq!(classify("vDisk", "Disk", &none), Class::Label);
        assert_eq!(classify("vDisk", "Disk Path", &none), Class::Path);
        assert_eq!(classify("vInfo", "Network #3", &none), Class::Hash);
        assert_eq!(classify("vLicense", "Key", &none), Class::Drop);
        assert_eq!(classify("vInfo", "Backup Owner", &none), Class::Custom);
        assert_eq!(classify("vMultiPath", "Path 2", &none), Class::Hash);
        assert_eq!(classify("vMultiPath", "Path 2 state", &none), Class::Keep);
        let keep: HashSet<String> = ["backup owner".to_string()].into();
        assert_eq!(classify("vInfo", "Backup Owner", &keep), Class::Keep);
    }
}
