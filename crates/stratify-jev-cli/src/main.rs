mod output;

use clap::{Parser, ValueEnum};
use jev_client::Client;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use stratify_jev_judge::cache::Cache;
use stratify_jev_judge::config::JevConfig;
use stratify_jev_judge::context::RepoContext;
use stratify_jev_judge::driver::Driver;
use stratify_jev_judge::model::{Confidence, Report, Severity};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Format {
    Human,
    Json,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum ConfLevel {
    Unknown,
    Likely,
    Certain,
}

impl From<ConfLevel> for Confidence {
    fn from(c: ConfLevel) -> Confidence {
        match c {
            ConfLevel::Unknown => Confidence::Unknown,
            ConfLevel::Likely => Confidence::Likely,
            ConfLevel::Certain => Confidence::Certain,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum FailOn {
    Never,
    Info,
    Warning,
    Error,
}

/// Judge a Stratify report with Jev. Reads the report on stdin.
#[derive(Parser)]
#[command(name = "stratify-jev", version)]
struct Args {
    /// Repository root, used for source reads and the file walk.
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// Read the report from a file instead of stdin.
    #[arg(long)]
    input: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,

    /// Hide findings below this confidence.
    #[arg(long, value_enum, default_value_t = ConfLevel::Likely)]
    min_confidence: ConfLevel,

    #[arg(long)]
    show_dismissed: bool,

    #[arg(long, value_enum, default_value_t = FailOn::Never)]
    fail_on: FailOn,

    /// Build every request, report the plan, and send nothing.
    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    no_cache: bool,

    #[arg(long, default_value = ".stratify/jev-cache")]
    cache_dir: PathBuf,

    #[arg(long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    let mut raw = String::new();
    let read = match &args.input {
        Some(p) => std::fs::read_to_string(p).map(|t| {
            raw = t;
        }),
        None => std::io::stdin().read_to_string(&mut raw).map(|_| ()),
    };
    if let Err(e) = read {
        eprintln!("stratify-jev: cannot read the report: {e}");
        return ExitCode::from(2);
    }

    let mut report: Report = match serde_json::from_str(&raw) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("stratify-jev: the input is not a Stratify JSON report: {e}");
            return ExitCode::from(2);
        }
    };

    if report.schema_version > Report::KNOWN_SCHEMA_VERSION {
        eprintln!(
            "stratify-jev: schema_version {} is newer than this build understands, \
             passing the report through unchanged",
            report.schema_version
        );
        print!("{}", render(&args, &report));
        return exit_code(&args, &report);
    }

    let ctx = match RepoContext::new(args.root.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("stratify-jev: cannot read {}: {e}", args.root.display());
            return ExitCode::from(2);
        }
    };
    let cfg = JevConfig::load(&args.root);

    if args.dry_run {
        let planned = plan_only(&report, &cfg);
        println!("{planned} request(s) planned, nothing sent.");
        return ExitCode::SUCCESS;
    }

    match Client::from_env() {
        None => {
            eprintln!(
                "stratify-jev: TYPESAFE_API_KEY is not set, passing the report through unchanged"
            );
        }
        Some(client) => {
            let cache = Cache::new(args.root.join(&args.cache_dir), !args.no_cache);
            let driver = Driver::new(Some(client), cache, cfg);
            let stats = driver.run(&mut report, &ctx).await;
            if args.verbose {
                eprintln!(
                    "stratify-jev: {} judged, {} from cache, {} request(s), {} failed, {} input tokens",
                    stats.judged,
                    stats.from_cache,
                    stats.requested,
                    stats.failed,
                    stats.input_tokens
                );
            }
            if stats.failed > 0 {
                eprintln!(
                    "stratify-jev: {} request(s) failed, those findings are unchanged",
                    stats.failed
                );
            }
        }
    }

    print!("{}", render(&args, &report));
    exit_code(&args, &report)
}

/// Count the requests a real run would send, without building a client.
fn plan_only(report: &Report, cfg: &JevConfig) -> usize {
    let claimed = report
        .findings
        .iter()
        .filter(|f| f.rule == "dead_code")
        .count();
    if claimed == 0 {
        return 0;
    }
    claimed.div_ceil(cfg.batch_findings.max(1))
}

fn render(args: &Args, report: &Report) -> String {
    match args.format {
        Format::Human => {
            output::human::render(report, args.min_confidence.into(), args.show_dismissed)
        }
        Format::Json => output::json::render(report, VERSION),
    }
}

/// Exit code follows --fail-on over post-judgment findings, so the tool
/// works as a gate without ever failing a build because TypeSafe was down.
fn exit_code(args: &Args, report: &Report) -> ExitCode {
    let floor = match args.fail_on {
        FailOn::Never => return ExitCode::SUCCESS,
        FailOn::Info => Severity::Info,
        FailOn::Warning => Severity::Warning,
        FailOn::Error => Severity::Error,
    };
    let hit = report.findings.iter().any(|f| {
        f.severity >= floor && output::human::visible(f, args.min_confidence.into(), args.show_dismissed)
    });
    if hit {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
