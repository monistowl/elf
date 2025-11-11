use anyhow::Result;
use clap::{Parser, Subcommand};
use elf_lib::{
    detectors::ecg::detect_r_peaks, metrics::hrv::hrv_time, signal::RRSeries, signal::TimeSeries,
};
use std::io::{self, Read};

#[derive(Parser)]
#[command(
    name = "elf",
    version,
    about = "ELF: Extensible Lab Framework CLI tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Detect R-peaks from newline-delimited samples read from stdin
    EcgFindRpeaks {
        #[arg(long, default_value_t = 250.0)]
        fs: f64,
        #[arg(long, default_value_t = 0.3)]
        min_rr_s: f64,
    },
    /// Compute time-domain HRV from newline-delimited RR intervals (seconds) read from stdin
    HrvTime,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::EcgFindRpeaks { fs, min_rr_s } => cmd_ecg_find_rpeaks(fs, min_rr_s)?,
        Commands::HrvTime => cmd_hrv_time()?,
    }
    Ok(())
}

fn read_stdin_f64s() -> Result<Vec<f64>> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    let mut out = Vec::new();
    for line in buf.lines() {
        let v: f64 = line.trim().parse()?;
        out.push(v);
    }
    Ok(out)
}

fn cmd_ecg_find_rpeaks(fs: f64, min_rr_s: f64) -> Result<()> {
    let data = read_stdin_f64s()?;
    let ts = TimeSeries { fs, data };
    let events = detect_r_peaks(&ts, min_rr_s);
    let js = serde_json::to_string(&events)?;
    println!("{}", js);
    Ok(())
}

fn cmd_hrv_time() -> Result<()> {
    let rr = read_stdin_f64s()?;
    let rr = RRSeries { rr };
    let m = hrv_time(&rr);
    let js = serde_json::to_string(&m)?;
    println!("{}", js);
    Ok(())
}
