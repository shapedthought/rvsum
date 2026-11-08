use std::collections::HashSet;
use std::path::PathBuf;

use clap::Parser;

use crate::summary::SummaryFormat;

#[derive(Parser, Debug)]
#[clap(author, version, about, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// RvTools File(s)
    #[clap(short, long, value_delimiter = ',', num_args = 1..)]
    pub rvtools_files: Vec<PathBuf>,

    /// Folder(s) of RvTools files - every .xlsx inside is used
    #[clap(short = 'D', long, value_delimiter = ',', num_args = 1..)]
    pub rvtools_dir: Vec<PathBuf>,

    /// Descend into subfolders of --rvtools-dir
    #[clap(long, action, default_value_t = false)]
    pub recursive: bool,

    /// Include Powered Off VMs
    #[clap(short, long, action, default_value_t = false)]
    pub include_powered_off: bool,

    /// Retention - example 30D1W1M1Y - global
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub retention: Option<String>,

    /// Output File [Optional]
    #[clap(short, long, value_parser)]
    pub output_file: Option<String>,

    /// Print converted data (VSE format)
    #[clap(short, long, action, default_value_t = false)]
    pub print: bool,

    /// Print the VM info to JSON
    #[clap(long, action, default_value_t = false)]
    pub print_json: bool,

    /// Print DC level summary
    #[clap(short, long, action, default_value_t = false)]
    pub show_info: bool,

    /// Output format for --show-info / --summary-file / --exceptions-file.
    /// If omitted, file outputs infer from the extension (.csv/.json/.md; otherwise table).
    #[clap(long, value_enum)]
    pub format: Option<SummaryFormat>,

    /// Write the --show-info summary to a file instead of stdout (implies --show-info)
    #[clap(long, value_parser)]
    pub summary_file: Option<PathBuf>,

    /// DC include list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub dc_include: Option<Vec<String>>,

    /// Cluster include list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub cluster_include: Option<Vec<String>>,

    /// DC exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub dc_exclude: Option<Vec<String>>,

    /// Cluster exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub cluster_exclude: Option<Vec<String>>,

    /// VM exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    pub vm_exclude: Option<Vec<String>>,

    /// Legacy mode - pre v4.1.2
    #[clap(long, action, default_value_t = false)]
    pub legacy: bool,

    /// Rename datacenters using a JSON object (`{"real-dc": "Display"}` or `{"Display": ["real-dc-1", "real-dc-2"]}`)
    #[clap(long, value_parser, conflicts_with = "dc_site_map")]
    pub dc_rename: Option<PathBuf>,

    /// Display names produced by `--dc-rename`; kept unhashed under `--anonymize`.
    #[clap(skip)]
    pub dc_rename_targets: HashSet<String>,

    /// Map DCs to a site - requires a JSON file
    #[clap(long, value_parser)]
    pub dc_site_map: Option<PathBuf>,

    /// Creates Map DC JSON template
    #[clap(long, action, default_value_t = false)]
    pub dc_site_map_template: bool,

    /// Don't use vPartition capacity
    #[clap(short, long, action, default_value_t = false)]
    pub do_not_use_vpartition: bool,

    /// Print DCs
    #[clap(long, action, default_value_t = false)]
    pub dc_print: bool,

    /// Print VM table
    #[clap(short, long, action, default_value_t = false)]
    pub vm_table_print: bool,

    /// Flatten to single site, repo and workload
    #[clap(long, action, default_value_t = false)]
    pub flatten: bool,

    /// Flatten to single cluster per-site
    #[clap(long, action, default_value_t = false)]
    pub flatten_site: bool,

    /// Plot capacity data in a bar chart
    #[clap(long, action, default_value_t = false)]
    pub plot: bool,

    /// Anonymize the data
    #[clap(long, action, default_value_t = false)]
    pub anonymize: bool,

    /// Keep physical-compatibility RDM capacity in the estimate (excluded by default)
    #[clap(long, action, default_value_t = false)]
    pub keep_rdm: bool,

    /// Keep independent-mode disk capacity in the estimate (excluded by default)
    #[clap(long, action, default_value_t = false)]
    pub keep_independent: bool,

    /// Flag possible in-guest storage when guest file systems exceed vDisks by this percentage
    #[clap(long, value_parser, default_value_t = 10.0)]
    pub guest_gap_threshold: f64,

    /// Minimum gap in GiB for the in-guest storage heuristic
    #[clap(long, value_parser, default_value_t = 50.0)]
    pub guest_gap_min_gib: f64,

    /// Write disk-awareness exceptions to a file (format from --format or file extension)
    #[clap(long, value_parser)]
    pub exceptions_file: Option<PathBuf>,

    /// Write a self-contained offline HTML capacity, coverage and customer action report
    #[clap(long, value_parser)]
    pub html_report: Option<PathBuf>,

    /// Write one customer action row per VM (.csv, .json, .md or .xlsx)
    #[clap(long, value_parser)]
    pub action_report: Option<PathBuf>,

    /// Write the cleaned per-VM and per-disk tables to vms.parquet and disks.parquet in this folder
    #[clap(long, value_parser)]
    pub export_parquet: Option<PathBuf>,

    /// Bypass reading and writing the parsed workbook cache
    #[clap(long, action, conflicts_with = "refresh_cache")]
    pub no_cache: bool,

    /// Rebuild the parsed workbook cache instead of reusing it
    #[clap(long, action)]
    pub refresh_cache: bool,

    /// Print the full disk-awareness exceptions list to stderr (default is a compact summary).
    /// Also report parsed workbook cache activity.
    /// Note: short `-v` is already `--vm-table-print`.
    #[clap(long, action, default_value_t = false)]
    pub verbose: bool,
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Write an anonymised copy of RVTools workbook(s) that is safe to share and still sizes identically
    #[command(visible_alias = "anonymize")]
    Anonymise(AnonymiseArgs),
}

#[derive(clap::Args, Debug)]
pub struct AnonymiseArgs {
    /// Input RVTools workbook(s); repeat or comma-separate for several files
    #[arg(short, long, required = true, value_delimiter = ',', num_args = 1..)]
    pub input: Vec<PathBuf>,

    /// Output .xlsx (one input) or folder (one or more inputs; writes NAME_anon.xlsx)
    #[arg(short, long)]
    pub output: PathBuf,

    /// Secret salt so tokens are reproducible across runs (random if omitted)
    #[arg(long, conflicts_with = "salt_file")]
    pub salt: Option<String>,

    /// Read the salt from a file (first line, trimmed)
    #[arg(long)]
    pub salt_file: Option<PathBuf>,

    /// Write original,token pairs to this CSV (SENSITIVE: keep it private; created mode 0600)
    #[arg(long)]
    pub mapping_file: Option<PathBuf>,

    /// Keep this column's header and values unchanged (case-insensitive; repeatable)
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    pub keep_column: Vec<String>,

    /// Overwrite existing output files
    #[arg(long, action, default_value_t = false)]
    pub force: bool,
}
