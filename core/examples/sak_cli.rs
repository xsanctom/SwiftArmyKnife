//! Tiny dev CLI for exercising the engine without the GUI.
//!
//!   cargo run --example sak_cli -- <file>            # probe + show menu
//!   cargo run --example sak_cli -- <file> <op_id>    # also run that op
//!
//! op_id: 0=Convert 1=Compress 2=ExtractAudio 3=Gif

use std::io::Write;
use std::sync::atomic::AtomicBool;
use swiss_army_knife_core::{menu_for, ops::JobParams, probe::probe, run_job_blocking};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(file) = args.get(1) else {
        eprintln!("usage: sak_cli <file> [op_id]");
        std::process::exit(2);
    };

    let p = probe("ffprobe", file);
    println!("probe   : {p:?}");
    println!("menu    : {:?}", menu_for(&p));

    let Some(op_id) = args.get(2).and_then(|s| s.parse::<u32>().ok()) else {
        return;
    };

    let cancel = AtomicBool::new(false);
    let result = run_job_blocking("ffmpeg", file, op_id, &JobParams::default(), &p, &cancel, |pr| {
        print!("\rprogress: {:>3.0}%  eta {:>4.1}s   ", pr.pct * 100.0, pr.eta_s);
        std::io::stdout().flush().ok();
    });
    match result {
        Ok(out) => println!("\noutput  : {}", out.display()),
        Err(e) => {
            eprintln!("\nerror   : {e}");
            std::process::exit(1);
        }
    }
}
