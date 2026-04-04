use std::env;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "--help" | "-h" => { usage(); return; }
        "--version" | "-V" => { version(); return; }
        _ => {}
    }

    // Parse: afs-as <input.s> -o <output.o>
    let input = &args[1];
    let output = if args.len() >= 4 && args[2] == "-o" {
        args[3].clone()
    } else {
        // Default: replace .s with .o
        let p = Path::new(input);
        p.with_extension("o").to_string_lossy().into_owned()
    };

    let input_path = Path::new(input);
    let output_path = Path::new(&output);

    if let Err(e) = afs_as::assemble::assemble_file(input_path, output_path) {
        eprintln!("afs-as: {}", e);
        process::exit(1);
    }
}

fn usage() {
    eprintln!("afs-as: ARM64 assembler for macOS");
    eprintln!();
    eprintln!("usage: afs-as <input.s> [-o <output.o>]");
    eprintln!("       afs-as --help");
    eprintln!("       afs-as --version");
}

fn version() {
    eprintln!("afs-as {}", env!("CARGO_PKG_VERSION"));
}
