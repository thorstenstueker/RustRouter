//! RustRouter command line: routes a Specctra DSN design and writes a Specctra SES session.
//!
//! The options follow the Freerouting CLI:
//!   -de <file>   input design (.dsn)
//!   -do <file>   output session (.ses)
//!   -mp <n>      maximum number of autorouter passes (0 = no limit)
//!   -mt <n>      maximum number of threads (optimizer)
//!   -oit <pct>   optimizer improvement threshold in percent
//!   -jt <time>   job timeout (HH:MM:SS)
//!   --no-optimizer, --no-fanout, --no-router

use rustrouter::autoroute::pipeline::{run_pipeline, stats, RoutingJob};
use rustrouter::io::specctra;
use rustrouter::settings::RouterSettings;
use std::process::ExitCode;

#[cfg(not(target_os = "macos"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use std::time::{Duration, Instant};

struct Options {
    input: Option<String>,
    output: Option<String>,
    max_passes: Option<i32>,
    max_threads: Option<i32>,
    improvement_threshold: Option<f32>,
    job_timeout: Option<String>,
    no_optimizer: bool,
    no_fanout: bool,
    no_router: bool,
}

fn usage() -> &'static str {
    "usage: rustrouter -de <input.dsn> -do <output.ses> [-mp <passes>] [-mt <threads>] [-oit <percent>] [-jt <HH:MM:SS>] [--no-optimizer] [--no-fanout] [--no-router]"
}

fn parse_args() -> Result<Options, String> {
    let mut o = Options {
        input: None,
        output: None,
        max_passes: None,
        max_threads: None,
        improvement_threshold: None,
        job_timeout: None,
        no_optimizer: false,
        no_fanout: false,
        no_router: false,
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let value = |i: &mut usize, name: &str| -> Result<String, String> {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| format!("missing value for {name}"))
    };
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-de" | "--design-input" => o.input = Some(value(&mut i, a)?),
            "-do" | "--design-output" => o.output = Some(value(&mut i, a)?),
            "-mp" | "--max-passes" => o.max_passes = Some(value(&mut i, a)?.parse().map_err(|e| format!("{a}: {e}"))?),
            "-mt" | "--max-threads" => o.max_threads = Some(value(&mut i, a)?.parse().map_err(|e| format!("{a}: {e}"))?),
            "-oit" | "--optimizer-improvement-threshold" => {
                o.improvement_threshold = Some(value(&mut i, a)?.parse().map_err(|e| format!("{a}: {e}"))?)
            }
            "-jt" | "--job-timeout" => o.job_timeout = Some(value(&mut i, a)?),
            "--no-optimizer" => o.no_optimizer = true,
            "--no-fanout" => o.no_fanout = true,
            "--no-router" => o.no_router = true,
            "-h" | "--help" => return Err(usage().to_string()),
            "-V" | "--version" => {
                println!("rustrouter {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown option '{other}'\n{}", usage())),
        }
        i += 1;
    }
    Ok(o)
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let Some(input) = opts.input.clone() else {
        eprintln!("{}", usage());
        return ExitCode::from(2);
    };
    let output = opts.output.clone().unwrap_or_else(|| {
        let p = std::path::Path::new(&input);
        p.with_extension("ses").to_string_lossy().to_string()
    });
    let started = Instant::now();
    log::info!("Loading board file '{input}'...");
    let dsn = match specctra::read_dsn(&input) {
        Ok(d) => d,
        Err(e) => {
            log::error!("There was an error while reading DSN file: {e:?}");
            return ExitCode::from(1);
        }
    };
    for w in &dsn.warnings {
        log::warn!("{w}");
    }
    let mut settings = RouterSettings::default();
    if let Some(mp) = opts.max_passes {
        settings.autorouter.max_passes = mp;
    }
    if let Some(mt) = opts.max_threads {
        settings.set_max_threads(Some(mt));
    }
    if let Some(t) = opts.improvement_threshold {
        settings.optimizer.optimization_improvement_threshold = t;
    }
    if let Some(t) = &opts.job_timeout {
        settings.job_timeout_string = t.clone();
    }
    if opts.no_optimizer {
        settings.optimizer.enabled = false;
    }
    if opts.no_fanout {
        settings.fanout.enabled = false;
    }
    if opts.no_router {
        settings.autorouter.enabled = false;
    }
    let mut board = dsn.board.clone();
    rustrouter::job::prepare_loaded_board(&dsn, &mut board, &mut settings);
    // Settings given on the command line take precedence over the DSN file.
    if opts.no_optimizer {
        settings.optimizer.enabled = false;
    }
    if opts.no_router {
        settings.autorouter.enabled = false;
    }
    settings.validate_against_board(Some(&board));
    // Like Freerouting, the session is named after the input file.
    let design_name = std::path::Path::new(&input)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| dsn.design_name.clone().unwrap_or_default());
    let mut job = RoutingJob::new(board, settings);
    if let Some(secs) = rustrouter::autoroute::pipeline::parse_timespan_string(&job.settings.job_timeout_string) {
        let secs = secs.min(24 * 60 * 60).max(0);
        job.start_timeout_monitor(Duration::from_secs(secs as u64));
    }
    run_pipeline(&mut job);
    let final_stats = stats::board_stats(&job.board, &job.settings);
    log::info!(
        "Routing finished in {:.2} seconds: {} unrouted connection(s), {} clearance violation(s), {} via(s), router score {:.2}.",
        started.elapsed().as_secs_f64(),
        final_stats.incomplete_count,
        final_stats.clearance_violation_count,
        final_stats.via_count,
        final_stats.router_score
    );
    if let Err(e) = specctra::write_ses_file(&job.board, &design_name, &output) {
        log::error!("unable to write session file '{output}': {e:?}");
        return ExitCode::from(1);
    }
    log::info!("Session file written to '{output}'.");
    ExitCode::SUCCESS
}
