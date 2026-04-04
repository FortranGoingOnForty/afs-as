use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: afs-as <input.s> -o <output.o>");
        process::exit(1);
    }
    // TODO: parse args, assemble, emit Mach-O
    eprintln!("afs-as: not yet implemented");
    process::exit(1);
}
