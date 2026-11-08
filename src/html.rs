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

// These colors are shared by the donut, stacked bars and legends. Labels and
// numeric totals also carry meaning when printed without background colors.
const COLORS: [&str; 3] = ["#2463a6", "#6e46a2", "#b46a16"];
const COVERAGE: [&str; 3] = ["Image-level", "Agent in estimate", "Excluded"];

fn coverage_values(t: &Totals) -> [f64; 3] {
    [t.image / TB, t.agent / TB, t.excluded / TB]
}

fn coverage_legend(out: &mut String) {
    out.push_str("<p class=legend>");
    for (label, color) in COVERAGE.iter().zip(COLORS) {
        write!(out, "<span style=\"color:{color}\">■ {label}</span>").unwrap();
    }
    out.push_str("</p>");
}

fn coverage_donut(out: &mut String, total: &Totals) {
    let values = coverage_values(total);
    let sum: f64 = values.iter().sum();
    out.push_str("<svg class=donut viewBox=\"0 0 760 260\" role=img aria-label=\"Coverage donut\"><title>Coverage split — TB</title><circle cx=130 cy=130 r=88 fill=none stroke=\"#e4ebf2\" stroke-width=\"32\"/>");
    let circumference = std::f64::consts::TAU * 88.0;
    let mut offset = 0.0;
    for ((label, value), color) in COVERAGE.iter().zip(values).zip(COLORS) {
        if value > 0.0 && sum > 0.0 {
            let length = value / sum * circumference;
            write!(out, "<circle cx=130 cy=130 r=88 fill=none stroke=\"{color}\" stroke-width=32 stroke-dasharray=\"{length:.4} {circumference:.4}\" stroke-dashoffset=\"{offset:.4}\" transform=\"rotate(-90 130 130)\"><title>{label}: {value:.2} TB</title></circle>").unwrap();
            offset -= length;
        }
    }
    write!(out, "<text x=130 y=126 text-anchor=middle class=donut-total>{sum:.2}</text><text x=130 y=151 text-anchor=middle>TB coverage</text>").unwrap();
    for (i, ((label, value), color)) in COVERAGE.iter().zip(values).zip(COLORS).enumerate() {
        let y = 75 + i * 58;
        write!(out, "<rect x=280 y=\"{}\" width=14 height=14 rx=3 fill=\"{color}\"/><text x=308 y=\"{y}\">{label}: {value:.2} TB</text>", y - 12).unwrap();
    }
    out.push_str("</svg>");
    if sum == 0.0 {
        out.push_str("<p>No capacity to chart.</p>");
    }
}

/// Deterministic descending ranking; all omitted clusters contribute to Other.
fn capacity_rows(groups: &BTreeMap<Vec<String>, Totals>, limit: usize) -> Vec<(String, [f64; 3])> {
    let mut ranked: Vec<_> = groups.iter().collect();
    ranked.sort_by(|(ka, a), (kb, b)| {
        let sum = |t: &Totals| t.image + t.agent + t.excluded;
        sum(b).total_cmp(&sum(a)).then_with(|| ka.cmp(kb))
    });
    let mut result = Vec::new();
    let mut other = Totals::default();
    for (i, (names, total)) in ranked.into_iter().enumerate() {
        if i < limit {
            // Put the cluster first so long vCenter names do not hide it.
            let label = names
                .iter()
                .rev()
                .map(|s| if s.is_empty() { "(unknown)" } else { s })
                .collect::<Vec<_>>()
                .join(" / ");
            result.push((label, coverage_values(total)));
        } else {
            other.add(total);
        }
    }
    if groups.len() > limit {
        result.push(("Other".into(), coverage_values(&other)));
    }
    result
}

/// Every row has a full tooltip and a numeric total; shortened visible labels
/// cannot clip adjacent bars. Only escaped text is derived from inventory.
/// Singular "VM" for a count of exactly one; TB otherwise unchanged.
fn unit_for(counts: bool, value: f64) -> &'static str {
    match (counts, value.round() == 1.0) {
        (true, true) => "VM",
        (true, false) => "VMs",
        (false, _) => "TB",
    }
}

fn bar_chart(out: &mut String, title: &str, rows: &[(String, [f64; 3])], counts: bool) {
    let max = rows
        .iter()
        .map(|(_, v)| v.iter().sum::<f64>())
        .fold(0.0, f64::max);
    // VM axes use whole counts, even when there is just one actionable VM.
    let max = if counts && max > 0.0 {
        (max / 4.0).ceil() * 4.0
    } else {
        max
    };
    let height = 56 + rows.len() * 46;
    let unit = if counts { "VMs" } else { "TB" };
    write!(out, "<svg class=bars viewBox=\"0 0 1000 {height}\" role=img aria-label=\"{}\"><title>{} — {unit}</title>", escape(title), escape(title)).unwrap();
    for tick in 0..=4 {
        let x = 350 + tick * 140;
        let value = max * tick as f64 / 4.0;
        let text = if counts {
            format!("{value:.0}")
        } else {
            format!("{value:.2}")
        };
        write!(out, "<path d=\"M{x} 30V{}\" stroke=\"#dce4ed\"/><text x=\"{x}\" y=20 text-anchor=middle class=axis>{text}</text>", height - 10).unwrap();
    }
    for (i, (label, values)) in rows.iter().enumerate() {
        let y = 42 + i * 46;
        let short = if label.chars().count() > 39 {
            format!("{}…", label.chars().take(38).collect::<String>())
        } else {
            label.clone()
        };
        write!(
            out,
            "<text x=330 y=\"{}\" text-anchor=end><title>{}</title>{}</text>",
            y + 18,
            escape(label),
            escape(&short)
        )
        .unwrap();
        let mut x = 350.0;
        for (j, value) in values.iter().enumerate() {
            let width = if max > 0.0 { value / max * 560.0 } else { 0.0 };
            let category = if counts { "Actions" } else { COVERAGE[j] };
            let number = if counts {
                format!("{value:.0}")
            } else {
                format!("{value:.2}")
            };
            let unit = unit_for(counts, *value);
            write!(out, "<rect x=\"{x:.4}\" y=\"{y}\" width=\"{width:.4}\" height=28 fill=\"{}\"><title>{} — {category}: {number} {unit}</title></rect>", COLORS[j], escape(label)).unwrap();
            x += width;
        }
        let sum: f64 = values.iter().sum();
        let number = if counts {
            format!("{sum:.0}")
        } else {
            format!("{sum:.2}")
        };
        let unit = unit_for(counts, sum);
        write!(
            out,
            "<text x=\"{:.4}\" y=\"{}\">{number} {unit}</text>",
            x + 10.0,
            y + 19
        )
        .unwrap();
    }
    out.push_str("</svg>");
    if max == 0.0 {
        out.push_str(if counts {
            "<p>No actions to chart.</p>"
        } else {
            "<p>No capacity to chart.</p>"
        });
    }
}

fn capacity_table(
    out: &mut String,
    title: &str,
    groups: &BTreeMap<Vec<String>, Totals>,
    labels: &[&str],
) {
    write!(out, "<section><h2>{title}</h2>").unwrap();
    out.push_str("<p class=note>Coverage capacity (not provisioned inventory): image-level + agent in estimate + excluded non-VADP. Hover for segment values; complete inventory totals are in the table below.</p>");
    let limit = if labels.len() > 1 { 15 } else { usize::MAX };
    if groups.len() > limit {
        out.push_str("<p class=note>Top 15 clusters by coverage capacity; remaining clusters combined as Other.</p>");
    }
    bar_chart(out, title, &capacity_rows(groups, limit), false);
    coverage_legend(out);
    out.push_str("<div class=scroll><table><thead><tr>");
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
    coverage_donut(&mut out, &total);
    out.push_str("</section>");
    capacity_table(&mut out, "Capacity by vCenter", &centers, &["vCenter"]);
    capacity_table(
        &mut out,
        "Capacity by cluster",
        &clusters,
        &["vCenter", "Datacenter", "Cluster"],
    );
    out.push_str("<section><h2>Action categories</h2><p>VM counts by highest-priority action category. Each actionable VM is counted once; VMs without actions are not shown.</p>");
    let mut counts = [0_usize; 3];
    for row in rows {
        let fields = actions::fields(row)?;
        if let Some(i) = actions::CATEGORIES.iter().position(|c| *c == fields[0]) {
            counts[i] += 1;
        }
    }
    let action_counts: Vec<_> = actions::CATEGORIES
        .iter()
        .zip(counts)
        .map(|(c, n)| (c.to_string(), [n as f64, 0.0, 0.0]))
        .collect();
    bar_chart(&mut out, "Action categories", &action_counts, true);
    out.push_str("</section>");
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
:root{font:16px system-ui,sans-serif;color:#182c42;background:#edf2f7}body{margin:0}header,main,footer{max-width:1400px;margin:auto;padding:2rem}header{background:#132c45;color:#fff}h1{font-size:2.5rem;margin:.5rem 0}h2{margin-top:0;font-size:1.4rem}.eyebrow{letter-spacing:.15em;font-size:.75rem;color:#b9d9f7}section{background:white;border:1px solid #dce4ed;border-radius:12px;padding:1.5rem;margin-bottom:1.5rem;box-shadow:0 2px 8px #152f4710}.kpis{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:1rem}article{background:#f0f5fa;padding:1.2rem;border-radius:8px}article strong{display:block;font-size:2rem;font-variant-numeric:tabular-nums}article span{font-size:.85rem}.note,footer{font-size:.85rem;color:#52667a;line-height:1.5}p{line-height:1.5}svg{width:100%;height:auto;display:block;color:#182c42}svg text{fill:currentColor;font:14px system-ui,sans-serif}.donut{max-width:900px;margin:auto}.donut text{font-size:18px}.donut .donut-total{font-size:30px;font-weight:700}.axis{fill:#52667a;font-size:12px}.bars{margin:1rem 0;break-inside:avoid}.legend{display:flex;flex-wrap:wrap;gap:1.5rem}.legend span:nth-child(1){color:#2463a6}.legend span:nth-child(2){color:#6e46a2}.legend span:nth-child(3){color:#9a570c}.scroll{overflow:auto}table{border-collapse:collapse;width:100%;font-size:.85rem}td,th{padding:.7rem;text-align:left;border-bottom:1px solid #dce4ed;vertical-align:top;min-width:100px}th{background:#edf2f7;position:sticky;top:0}tbody tr:nth-child(even){background:#f8fafc}td{white-space:pre-wrap;overflow-wrap:anywhere}button{border:0;background:none;font:inherit;font-weight:700;color:inherit;cursor:pointer;text-align:left}button:focus-visible,input:focus-visible,select:focus-visible{outline:3px solid #2463a6}th[aria-sort=ascending] button:after{content:' ↑'}th[aria-sort=descending] button:after{content:' ↓'}.controls{display:flex;align-items:center;gap:1rem;flex-wrap:wrap}label{display:flex;align-items:center;gap:.5rem}input,select{padding:.6rem;border:1px solid #9cafbf;border-radius:6px;font:inherit;max-width:100%}[hidden]{display:none!important}@media(max-width:600px){header,main,footer{padding:1rem}section{padding:1rem}h1{font-size:2rem}}@media print{svg{print-color-adjust:exact;-webkit-print-color-adjust:exact}body{background:white}header{color:#182c42;background:white}.controls,button:after{display:none}section{box-shadow:none;break-inside:avoid}.scroll{overflow:visible}table{font-size:8pt}td,th{min-width:0;padding:3pt}#actions{table-layout:fixed}button{padding:0}header,main,footer{padding:0}}
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
    #[test]
    fn donut_values_include_all_categories_and_zero_is_finite() {
        let mut out = String::new();
        coverage_donut(
            &mut out,
            &Totals {
                image: TB,
                agent: 2.0 * TB,
                excluded: 3.0 * TB,
                ..Totals::default()
            },
        );
        assert!(out.contains("Image-level: 1.00 TB"));
        assert!(out.contains("Agent in estimate: 2.00 TB"));
        assert!(out.contains("Excluded: 3.00 TB"));
        assert!(out.contains(">6.00</text>"));
        assert!(out.contains("stroke-dasharray=\"92.1534 552.9203\""));
        out.clear();
        coverage_donut(&mut out, &Totals::default());
        assert!(out.contains("No capacity to chart."));
        assert!(!out.contains("NaN") && !out.contains("inf"));
    }

    #[test]
    fn top_clusters_other_preserves_each_coverage_category_and_ties() {
        let groups = (0..18)
            .map(|i| {
                (
                    vec![format!("cluster-{i:02}")],
                    Totals {
                        image: TB,
                        agent: 2.0 * TB,
                        excluded: 3.0 * TB,
                        ..Totals::default()
                    },
                )
            })
            .collect();
        let rows = capacity_rows(&groups, 15);
        assert_eq!(rows.len(), 16);
        assert_eq!(rows[0].0, "cluster-00");
        assert_eq!(rows[14].0, "cluster-14");
        assert_eq!(rows[15], ("Other".into(), [3.0, 6.0, 9.0]));
        let all = capacity_rows(&groups, usize::MAX);
        assert_eq!(all.len(), 18);
        assert_eq!(
            all.iter().map(|(_, v)| v.iter().sum::<f64>()).sum::<f64>(),
            108.0
        );
    }

    #[test]
    fn bars_escape_titles_labels_and_render_exact_values() {
        let mut out = String::new();
        bar_chart(
            &mut out,
            "<capacity>",
            &[("<&>\"'".into(), [1.0, 2.0, 3.0])],
            false,
        );
        assert!(out.contains("aria-label=\"&lt;capacity&gt;\""));
        assert!(out.contains("&lt;&amp;&gt;&quot;&#39; — Agent in estimate: 2.00 TB"));
        assert!(out.contains("width=\"186.6667\""));
        assert!(out.contains(">6.00 TB</text>"));
        assert!(!out.contains("<&>"));
        out.clear();
        bar_chart(&mut out, "Empty", &[], false);
        assert!(out.contains("No capacity to chart."));
        assert!(!out.contains("NaN"));
        out.clear();
        bar_chart(
            &mut out,
            "Actions",
            &[("Category".into(), [7.0, 0.0, 0.0])],
            true,
        );
        assert!(out.contains("Category — Actions: 7 VMs"));
        assert!(out.contains(">7 VMs</text>"));
    }

    #[test]
    fn vm_unit_is_singular_for_one() {
        assert_eq!(unit_for(true, 1.0), "VM");
        assert_eq!(unit_for(true, 2.0), "VMs");
        assert_eq!(unit_for(true, 0.0), "VMs");
        assert_eq!(unit_for(false, 1.0), "TB");
    }
}
