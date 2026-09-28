mod output;

use clap::{Parser, ValueEnum};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use stratify_judge_core::backend::{client_for, resolve_backend};
use stratify_judge_core::cache::Cache;
use stratify_judge_core::config::JudgeConfig;
use stratify_judge_core::context::RepoContext;
use stratify_judge_core::driver::Driver;
use stratify_judge_core::model::{Confidence, Report, Severity};

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

/// Judge a Stratify report with a System One model. Reads the report on
/// stdin.
#[derive(Parser)]
#[command(name = "stratify-judge", version)]
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

    /// Which model endpoint to ask: a built-in preset (jev, laya) or a
    /// name with a [judge.backends.<name>] table. Defaults to the
    /// [judge] backend key in config, then jev, when not passed.
    #[arg(long)]
    backend: Option<String>,

    /// Model id to send. Jev requires one; Laya ignores it.
    #[arg(long)]
    model: Option<String>,

    /// Override the API base URL, for pointing at a capture proxy or a
    /// local mock when diagnosing a live run.
    #[arg(long)]
    base_url: Option<String>,

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
        eprintln!("stratify-judge: cannot read the report: {e}");
        return ExitCode::from(2);
    }

    let mut report: Report = match serde_json::from_str(&raw) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("stratify-judge: the input is not a Stratify JSON report: {e}");
            return ExitCode::from(2);
        }
    };

    if report.schema_version > Report::KNOWN_SCHEMA_VERSION {
        eprintln!(
            "stratify-judge: schema_version {} is newer than this build understands, \
             passing the report through unchanged",
            report.schema_version
        );
        print!("{}", render(&args, &report));
        return exit_code(&args, &report);
    }

    // A root we cannot read means judgment cannot run. It does not mean the
    // engine's findings stop being valid, so the report still goes out and
    // --fail-on still gates on it, exactly like the missing-key path. Doing
    // otherwise would let a misconfigured checkout path hard-fail a build
    // that asked for --fail-on never, and would hand a downstream SARIF
    // converter an empty pipe.
    let ctx = match RepoContext::new(args.root.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "stratify-judge: cannot read {}: {e}. Passing the report through \
                 unchanged, nothing was judged",
                args.root.display()
            );
            print!("{}", render(&args, &report));
            return exit_code(&args, &report);
        }
    };
    let cfg = JudgeConfig::load(&args.root);
    // Resolved against the current directory, not --root: the cache is this
    // tool's own bookkeeping, and joining it onto --root would write
    // untracked files into whatever repository is being analysed.
    let cache = Cache::new(args.cache_dir.clone(), !args.no_cache);

    // Resolution order, narrowest wins: the flag, then the [judge] backend
    // key in config, then jev. The flag has to stay Option<String> with no
    // clap default, or "typed --backend jev" would be indistinguishable
    // from "typed nothing" and could never lose to a config key.
    let backend_name = args
        .backend
        .clone()
        .or_else(|| cfg.backend.clone())
        .unwrap_or_else(|| "jev".to_string());

    // An unresolvable backend is a configuration error discovered before
    // anything is read from any model, unlike every other failure below,
    // which passes the report through and lets --fail-on decide. So this
    // one exits loudly instead of passing through.
    let backend = match resolve_backend(
        &backend_name,
        &cfg,
        args.base_url.as_deref(),
        args.model.as_deref(),
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("stratify-judge: {e}");
            return ExitCode::from(2);
        }
    };

    if args.dry_run {
        // Counted through the same prepare-and-cache-split path a real run
        // takes, so a committed cache is reflected. A preview that ignores
        // the cache overstates cost in exactly the steady state the README
        // recommends.
        match Driver::new(None, cache, cfg, backend.clone()).plan(&report, &ctx) {
            Ok((planned, tokens)) => {
                println!(
                    "backend {} at {}: {planned} request(s) planned, \
                     {tokens} tokens estimated, nothing sent.",
                    backend.name, backend.url
                );
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                // A preview gates nothing: it always exits 0, per the
                // README's own contract, since dry-run sends nothing and
                // has nothing to fail a build over.
                eprintln!("stratify-judge: {e}");
                print!("{}", render(&args, &report));
                return ExitCode::SUCCESS;
            }
        }
    }

    let client = client_for(&backend);
    match client {
        None => {
            eprintln!(
                "stratify-judge: {} is not set, passing the report through unchanged",
                backend.api_key_env
            );
        }
        Some(client) => {
            let driver = Driver::new(Some(client), cache, cfg, backend.clone());
            let stats = match driver.run(&mut report, &ctx).await {
                Ok(stats) => stats,
                Err(failure) => {
                    // Same pass-through rule as every other failure here: a
                    // context floor stops judging, not the report from
                    // reaching the caller or --fail-on from gating on it.
                    // Falling through instead of returning here, with the
                    // stats the floor had already earned, is what keeps
                    // --verbose, the --root mismatch warning and the
                    // per-error detail block below running exactly as they
                    // do on any other failure path.
                    eprintln!("stratify-judge: {failure}");
                    failure.stats
                }
            };
            if args.verbose {
                eprintln!(
                    "stratify-judge: {} judged, {} from cache, {} request(s), {} failed, {} input tokens",
                    stats.judged,
                    stats.from_cache,
                    stats.requested,
                    stats.failed,
                    stats.input_tokens
                );
            }
            // C2: a --root that exists but does not match the report being
            // judged makes every span unreadable, which reads to the model
            // as "no caller anywhere" and produces confident garbage. Warn
            // before that garbage gets cached, without aborting: a repo
            // with genuinely generated or moved files should still work.
            if stats.prepared > 0 && stats.missing_sources * 2 > stats.prepared {
                eprintln!(
                    "stratify-judge: {} of {} findings name files that do not exist under {}; \
                     is --root correct?",
                    stats.missing_sources,
                    stats.prepared,
                    ctx.root().display()
                );
            }
            // C1: every ClientError has a distinct, useful message; print
            // requests, findings, and each one, instead of a bare count
            // that makes a bad key indistinguishable from a timeout.
            if stats.failed > 0 {
                eprintln!(
                    "stratify-judge: {} request(s) failed, {} finding(s) unjudged and left \
                     unchanged",
                    stats.failed, stats.unjudged
                );
                for e in &stats.errors {
                    if e == "invalid or missing API key" {
                        eprintln!(
                            "stratify-judge: error: {e} (retrying will not help; check \
                             {})",
                            backend.api_key_env
                        );
                    } else {
                        eprintln!("stratify-judge: error: {e}");
                    }
                }
            }
        }
    }

    print!("{}", render(&args, &report));
    exit_code(&args, &report)
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
///
/// Gates on severity and confidence directly, never on `show_dismissed`:
/// that flag only changes what human output displays, and a CI job adding
/// it for fuller logs must not start failing on findings the model
/// dismissed.
fn exit_code(args: &Args, report: &Report) -> ExitCode {
    let floor = match args.fail_on {
        FailOn::Never => return ExitCode::SUCCESS,
        FailOn::Info => Severity::Info,
        FailOn::Warning => Severity::Warning,
        FailOn::Error => Severity::Error,
    };
    let min_confidence: Confidence = args.min_confidence.into();
    let hit = report
        .findings
        .iter()
        .any(|f| f.severity >= floor && f.confidence >= min_confidence);
    if hit {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
