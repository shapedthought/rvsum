//! Offline customer report. Inventory is written only as escaped HTML text,
//! never interpolated into JavaScript, CSS, URLs or event-handler attributes.
use crate::{
    actions,
    disk::DiskAwarenessResult,
    models::{
        cli::Cli,
        rvtools::{Vdisk, Vinfo},
    },
};
use anyhow::Result;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write as _,
    fs,
    path::Path,
};

const TB: f64 = 1024.0 * 1024.0;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Default)]
struct Totals {
    vms: usize,
    provisioned: f64,
    unknown: usize,
    used: f64,
    image: f64,
    agent: f64,
    excluded: f64,
}
impl Totals {
    fn add(&mut self, other: &Self) {
        self.vms += other.vms;
        self.provisioned += other.provisioned;
        self.unknown += other.unknown;
        self.used += other.used;
        self.image += other.image;
        self.agent += other.agent;
        self.excluded += other.excluded;
    }
    fn cells(&self) -> String {
        format!(
            "<td>{}</td><td>{:.2}{}</td><td>{:.2}</td><td>{:.2}</td><td>{:.2}</td><td>{:.2}</td>",
            self.vms,
            self.provisioned / TB,
            if self.unknown > 0 { " *" } else { "" },
            self.used / TB,
            self.image / TB,
            self.agent / TB,
            self.excluded / TB
        )
    }
}

fn capacity_table(
    out: &mut String,
    title: &str,
    groups: &BTreeMap<Vec<String>, Totals>,
    labels: &[&str],
) {
    write!(
        out,
        "<section><h2>{title}</h2><div class=scroll><table><thead><tr>"
    )
    .unwrap();
    for label in labels {
        write!(out, "<th scope=col>{label}</th>").unwrap();
    }
    out.push_str("<th scope=col>VMs</th><th scope=col>Provisioned TB</th><th scope=col>Reported used TB</th><th scope=col>Image-level TB</th><th scope=col>Agent in estimate TB</th><th scope=col>Excluded TB</th></tr></thead><tbody>");
    for (names, total) in groups {
        out.push_str("<tr>");
        for name in names {
            write!(out, "<td>{}</td>", escape(name)).unwrap();
        }
        out.push_str(&total.cells());
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div></section>");
}

pub fn write(
    path: &Path,
    infos: &[Vinfo],
    disks: &[Vdisk],
    coverage: &DiskAwarenessResult,
    rows: &[actions::Action],
    cli: &Cli,
) -> Result<()> {
    let hide = |s: &str| {
        if cli.anonymize && !s.is_empty() {
            crate::anon(s)
        } else {
            s.to_owned()
        }
    };
    let mut attached: HashMap<&str, f64> = HashMap::new();
    let mut disk_centers: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for d in disks {
        *attached.entry(&d.vm_key).or_default() += d.capacity;
        if !d.vi_sdk.trim().is_empty() && !d.vi_sdk.eq_ignore_ascii_case("none") {
            disk_centers
                .entry(&d.vm_key)
                .or_default()
                .insert(d.vi_sdk.trim());
        }
    }
    let included: HashMap<_, _> = coverage
        .combined
        .iter()
        .map(|v| (v.vm_key.as_str(), v))
        .collect();
    let mut excluded: HashMap<&str, f64> = HashMap::new();
    for d in &coverage.excluded {
        *excluded.entry(&d.vm_key).or_default() += d.capacity_mib;
    }
    let (mut centers, mut clusters) = (
        BTreeMap::<Vec<String>, Totals>::new(),
        BTreeMap::<Vec<String>, Totals>::new(),
    );
    let mut total = Totals::default();
    for v in infos {
        let provisioned = v
            .provisioned
            .or_else(|| attached.get(v.vm_key.as_str()).copied());
        let inc = included.get(v.vm_key.as_str());
        let t = Totals {
            vms: 1,
            provisioned: provisioned.unwrap_or_default(),
            unknown: usize::from(provisioned.is_none()),
            used: v.capacity,
            image: inc.map_or(0.0, |v| v.capacity - v.non_vadp_capacity),
            agent: inc.map_or(0.0, |v| v.non_vadp_capacity),
            excluded: excluded.get(v.vm_key.as_str()).copied().unwrap_or_default(),
        };
        let center = if v.vcenter.is_empty() {
            disk_centers
                .get(v.vm_key.as_str())
                .filter(|s| s.len() == 1)
                .and_then(|s| s.first().copied())
                .unwrap_or_default()
        } else {
            &v.vcenter
        };
        let vc = if center.is_empty() {
            "Unknown vCenter".to_owned()
        } else {
            hide(center)
        };
        centers.entry(vec![vc.clone()]).or_default().add(&t);
        clusters
            .entry(vec![vc, hide(&v.datacenter), hide(&v.cluster)])
            .or_default()
            .add(&t);
        total.add(&t);
    }
    let mut out = String::from(HEADER);
    out.push_str("<header><p class=eyebrow>RVSUM · OFFLINE CUSTOMER REPORT</p><h1>Backup coverage &amp; capacity</h1><p>Plan protection for the whole environment, not just the snapshot-friendly disks.</p></header><main><section><h2>At a glance</h2><div class=kpis>");
    for (label, value) in [
        ("VMs in selected inventory", total.vms.to_string()),
        (
            "Provisioned TB",
            format!(
                "{:.2}{}",
                total.provisioned / TB,
                if total.unknown > 0 { " *" } else { "" }
            ),
        ),
        ("Reported used TB", format!("{:.2}", total.used / TB)),
        (
            "Image-level in estimate TB",
            format!("{:.2}", total.image / TB),
        ),
        ("Agent in estimate TB", format!("{:.2}", total.agent / TB)),
        (
            "Excluded non-VADP TB",
            format!("{:.2}", total.excluded / TB),
        ),
    ] {
        write!(
            out,
            "<article><strong>{value}</strong><span>{label}</span></article>"
        )
        .unwrap();
    }
    out.push_str("</div><p class=note>TB uses 1,048,576 MiB (binary TiB), matching rvsum. Used is vInfo In Use, not guest free-space data. Provisioned is vInfo Provisioned when available, otherwise the sum of attached vDisks; shared attachments can appear on several VMs. These inventory totals are not repository sizing.</p>");
    if total.unknown > 0 {
        write!(out, "<p class=note>* Provisioned capacity unavailable for {} VM(s); shown totals include known capacity only.</p>", total.unknown).unwrap();
    }
    write!(out, "<p class=note>Options: powered-off VMs {}; physical RDMs {}; independent disks {}; guest partition sizing {}. Inventory respects selection filters. Grouping below uses original inventory placement, not flatten/site-map labels.</p>",
        if cli.include_powered_off { "included" } else { "excluded" },
        if cli.keep_rdm { "kept" } else { "excluded" }, if cli.keep_independent { "kept" } else { "excluded" },
        if cli.do_not_use_vpartition { "disabled" } else { "enabled" }).unwrap();
    out.push_str("</section><section><h2>Coverage split</h2><p>Image-level and agent values are included in the estimate. Excluded storage still needs an agent or another protection method. Shared storage is counted once by the sizing logic. Keeping a disk does not make it snapshot-backupable.</p>");
    let sum = total.image + total.agent + total.excluded;
    out.push_str("<svg viewBox=\"0 0 100 10\" role=img aria-label=\"Coverage split: image-level, agent in estimate, excluded\"><title>Coverage split</title>");
    let mut x = 0.0;
    for (value, color) in [
        (total.image, "#2463a6"),
        (total.agent, "#6e46a2"),
        (total.excluded, "#b46a16"),
    ] {
        let width = if sum > 0.0 {
            (value / sum * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        write!(
            out,
            "<rect x=\"{x:.4}\" y=\"0\" width=\"{width:.4}\" height=\"10\" fill=\"{color}\"/>"
        )
        .unwrap();
        x += width;
    }
    out.push_str("</svg>");
    write!(out, "<p class=legend><span>■ Image-level: {:.2} TB</span><span>◆ Agent in estimate: {:.2} TB</span><span>▲ Excluded: {:.2} TB</span></p>",total.image/TB,total.agent/TB,total.excluded/TB).unwrap();
    if sum == 0.0 {
        out.push_str("<p>No capacity to chart.</p>");
    }
    out.push_str("</section>");
    capacity_table(&mut out, "Capacity by vCenter", &centers, &["vCenter"]);
    capacity_table(
        &mut out,
        "Capacity by cluster",
        &clusters,
        &["vCenter", "Datacenter", "Cluster"],
    );
    out.push_str("<section><h2>Customer actions</h2><p>One row per VM needing attention, using the same categories and recommendations as --action-report. No actions does not guarantee backup readiness.</p><div class=controls><label>Filter <input id=filter type=search placeholder=\"VM, cluster or issue\"></label><label>Category <select id=category><option value=\"\">All categories</option>");
    for c in actions::CATEGORIES {
        write!(out, "<option>{}</option>", escape(c)).unwrap();
    }
    write!(out,"</select></label><p id=count role=status>{} actions</p></div><p class=note>Click a column heading to sort. All rows remain readable without JavaScript.</p><div class=scroll><table id=actions><thead><tr>",rows.len()).unwrap();
    for h in actions::HEADERS {
        write!(
            out,
            "<th scope=col aria-sort=none><button type=button>{}</button></th>",
            escape(&h.replace('_', " "))
        )
        .unwrap();
    }
    out.push_str("</tr></thead><tbody>");
    for row in rows {
        out.push_str("<tr>");
        for field in actions::fields(row)? {
            write!(out, "<td>{}</td>", escape(&field)).unwrap();
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div>");
    if rows.is_empty() {
        out.push_str("<p>No customer actions found in this selection.</p>");
    }
    out.push_str("</section></main><footer>Generated by rvsum · Review with the customer before designing backup jobs. This file may contain sensitive inventory. Anonymisation hides identifiers, not capacity or topology.</footer>");
    out.push_str(SCRIPT);
    out.push_str("</body></html>\n");
    fs::write(path, out)?;
    Ok(())
}

const HEADER: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>rvsum — Backup coverage &amp; capacity</title><style>
:root{font:16px system-ui,sans-serif;color:#182c42;background:#edf2f7}body{margin:0}header,main,footer{max-width:1400px;margin:auto;padding:2rem}header{background:#132c45;color:#fff}h1{font-size:2.5rem;margin:.5rem 0}h2{margin-top:0;font-size:1.4rem}.eyebrow{letter-spacing:.15em;font-size:.75rem;color:#b9d9f7}section{background:white;border:1px solid #dce4ed;border-radius:12px;padding:1.5rem;margin-bottom:1.5rem;box-shadow:0 2px 8px #152f4710}.kpis{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:1rem}article{background:#f0f5fa;padding:1.2rem;border-radius:8px}article strong{display:block;font-size:2rem;font-variant-numeric:tabular-nums}article span{font-size:.85rem}.note,footer{font-size:.85rem;color:#52667a;line-height:1.5}p{line-height:1.5}svg{width:100%;height:72px;display:block}.legend{display:flex;flex-wrap:wrap;gap:1.5rem}.legend span:nth-child(1){color:#2463a6}.legend span:nth-child(2){color:#6e46a2}.legend span:nth-child(3){color:#9a570c}.scroll{overflow:auto}table{border-collapse:collapse;width:100%;font-size:.85rem}td,th{padding:.7rem;text-align:left;border-bottom:1px solid #dce4ed;vertical-align:top;min-width:100px}th{background:#edf2f7;position:sticky;top:0}tbody tr:nth-child(even){background:#f8fafc}td{white-space:pre-wrap;overflow-wrap:anywhere}button{border:0;background:none;font:inherit;font-weight:700;color:inherit;cursor:pointer;text-align:left}button:focus-visible,input:focus-visible,select:focus-visible{outline:3px solid #2463a6}th[aria-sort=ascending] button:after{content:' ↑'}th[aria-sort=descending] button:after{content:' ↓'}.controls{display:flex;align-items:center;gap:1rem;flex-wrap:wrap}label{display:flex;align-items:center;gap:.5rem}input,select{padding:.6rem;border:1px solid #9cafbf;border-radius:6px;font:inherit;max-width:100%}[hidden]{display:none!important}@media(max-width:600px){header,main,footer{padding:1rem}section{padding:1rem}h1{font-size:2rem}}@media print{body{background:white}header{color:#182c42;background:white}.controls,button:after{display:none}section{box-shadow:none;break-inside:avoid}.scroll{overflow:visible}table{font-size:8pt}td,th{min-width:0;padding:3pt}#actions{table-layout:fixed}button{padding:0}header,main,footer{padding:0}}
</style></head><body>"#;

const SCRIPT: &str = r#"<script>
'use strict';
const table=document.getElementById('actions'),body=table.tBodies[0];
const filter=document.getElementById('filter'),category=document.getElementById('category');
function apply(){const q=filter.value.toLocaleLowerCase();let n=0;for(const r of body.rows){r.hidden=!(r.textContent.toLocaleLowerCase().includes(q)&&(!category.value||r.cells[0].textContent===category.value));if(!r.hidden)n++;}document.getElementById('count').textContent=n+' actions shown';}
filter.addEventListener('input',apply);category.addEventListener('change',apply);
for(const [i,th] of Array.from(table.tHead.rows[0].cells).entries()){th.querySelector('button').addEventListener('click',()=>{const asc=th.getAttribute('aria-sort')!=='ascending';for(const h of table.tHead.rows[0].cells)h.setAttribute('aria-sort','none');th.setAttribute('aria-sort',asc?'ascending':'descending');const rows=Array.from(body.rows);rows.sort((a,b)=>{const x=a.cells[i].textContent,y=b.cells[i].textContent;const cmp=(i===10||i===11)?Number(x)-Number(y):x.localeCompare(y,undefined,{numeric:true,sensitivity:'base'});return asc?cmp:-cmp;});const fragment=document.createDocumentFragment();for(const r of rows)fragment.appendChild(r);body.appendChild(fragment);});}
</script>"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_all_html_delimiters() {
        assert_eq!(escape("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
    }
}
