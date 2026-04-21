use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use afs_as::{assemble, macho};

fn perf_source(blocks: usize) -> String {
    let mut src = String::new();
    src.push_str(".build_version macos, 11, 0 sdk_version 15, 5\n");
    src.push_str(".subsections_via_symbols\n");
    src.push_str(".globl _perf_entry\n");
    src.push_str(".text\n");
    src.push_str(".p2align 2\n");
    src.push_str("_perf_entry:\n");
    src.push_str("  mov x9, x0\n");

    for index in 0..blocks {
        src.push_str(&format!("Lblock_{}:\n", index));
        src.push_str("  and w8, w0, #0xff\n");
        src.push_str(&format!(
            "  ubfiz w10, w8, #{}, #{}\n",
            index % 12,
            4 + (index % 8)
        ));
        src.push_str("  msub w11, w10, w8, w0\n");
        src.push_str(&format!("  add x9, x9, #{}\n", 1 + (index % 128)));
        src.push_str(&format!("  sub x12, x9, #{}\n", 1 + ((index * 3) % 96)));
        src.push_str(&format!("  add x13, x12, w11, uxtw #{}\n", index % 3));
        src.push_str(&format!("  adrp x14, cstr{}_perf@PAGE\n", index));
        src.push_str(&format!("  add x14, x14, cstr{}_perf@PAGEOFF\n", index));
        src.push_str(&format!("  adrp x15, _ext_{}@GOTPAGE\n", index));
        src.push_str(&format!("  ldr x15, [x15, _ext_{}@GOTPAGEOFF]\n", index));
        src.push_str(&format!("  cbz w11, Lskip_{}\n", index));
        src.push_str(&format!("  tbz x13, #{}, Ldone_{}\n", index % 32, index));
        src.push_str("  dmb ish\n");
        src.push_str(&format!("Lskip_{}:\n", index));
        src.push_str("  isb sy\n");
        src.push_str(&format!("Ldone_{}:\n", index));
    }
    src.push_str("  ret\n");

    src.push_str(".section __TEXT,__cstring,cstring_literals\n");
    for index in 0..blocks {
        src.push_str(&format!("cstr{}_perf:\n", index));
        src.push_str(&format!("  .asciz \"perf-{:04}-payload\"\n", index));
    }

    src.push_str(".section __TEXT,__const\n");
    src.push_str(".p2align 3\n");
    for index in 0..blocks {
        src.push_str(&format!("const{}_perf:\n", index));
        match index % 4 {
            0 => src.push_str(&format!("  .quad data{}_perf\n", index)),
            1 => src.push_str(&format!("  .quad _ext_{}\n", index)),
            2 => src.push_str(&format!(
                "  .quad _other_{} - _ext_{} + {}\n",
                index,
                index,
                4 * (1 + (index % 4))
            )),
            _ => src.push_str("  .quad _puts@GOT\n"),
        }
    }

    src.push_str(".data\n");
    src.push_str(".p2align 3\n");
    for index in 0..blocks {
        src.push_str(&format!("data{}_perf:\n", index));
        src.push_str(&format!("  .quad cstr{}_perf\n", index));
        src.push_str(&format!("  .quad _ext_{}\n", index));
        src.push_str(&format!(
            "  .quad _other_{} - _ext_{} + {}\n",
            index,
            index,
            8 + ((index % 3) * 4)
        ));
    }

    src.push_str(&format!(
        ".zerofill __DATA,__bss,_perf_scratch,{},4\n",
        blocks * 16
    ));
    src
}

fn best_duration(samples: Vec<Duration>) -> Duration {
    samples.into_iter().min().expect("at least one sample")
}

fn measure_library_pass(src: &str, rounds: usize) -> Duration {
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let start = Instant::now();
        let obj = assemble::assemble_source(src).expect("assemble perf source");
        let mut bytes = Vec::new();
        macho::write_macho(&obj, &mut bytes).expect("write perf object");
        black_box(bytes.len());
        samples.push(start.elapsed());
    }
    best_duration(samples)
}

fn run_cli(bin: &Path, input: &Path, output: &Path) -> Duration {
    let start = Instant::now();
    let status = Command::new(bin)
        .arg(input)
        .arg("-o")
        .arg(output)
        .status()
        .expect("run afs-as cli");
    assert!(
        status.success(),
        "afs-as CLI failed for {}",
        input.display()
    );
    start.elapsed()
}

fn run_system_as(input: &Path, output: &Path) -> Duration {
    let start = Instant::now();
    let status = Command::new("as")
        .arg("-o")
        .arg(output)
        .arg(input)
        .status()
        .expect("run system as");
    assert!(status.success(), "system as failed for {}", input.display());
    start.elapsed()
}

fn best_cli_duration<F>(rounds: usize, mut f: F) -> Duration
where
    F: FnMut(usize) -> Duration,
{
    let mut samples = Vec::with_capacity(rounds);
    for round in 0..rounds {
        samples.push(f(round));
    }
    best_duration(samples)
}

#[test]
fn library_scaling_stays_reasonable_on_large_generated_input() {
    let medium = perf_source(96);
    let large = perf_source(192);

    let _ = measure_library_pass(&medium, 1);
    let _ = measure_library_pass(&large, 1);

    let medium_time = measure_library_pass(&medium, 2);
    let large_time = measure_library_pass(&large, 2);

    assert!(
        large_time <= medium_time.mul_f64(5.5),
        "library scaling regressed: medium {:?}, large {:?}",
        medium_time,
        large_time
    );
}

#[test]
fn cli_throughput_stays_under_generous_absolute_cap() {
    let src = perf_source(192);
    let tmp = std::env::temp_dir().join(format!(
        "afs_perf_sanity_{}_{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("main")
    ));
    fs::create_dir_all(&tmp).expect("create perf temp dir");
    let asm = tmp.join("perf.s");
    fs::write(&asm, &src).expect("write perf source");

    let afs_bin = Path::new(env!("CARGO_BIN_EXE_afs-as"));

    let warm_ours = tmp.join("warm_ours.o");
    let warm_ref = tmp.join("warm_ref.o");
    let _ = run_cli(afs_bin, &asm, &warm_ours);
    let _ = run_system_as(&asm, &warm_ref);
    assert!(warm_ours.metadata().expect("warm afs-as metadata").len() > 0);
    assert!(warm_ref.metadata().expect("warm system metadata").len() > 0);

    let ours = best_cli_duration(2, |round| {
        run_cli(afs_bin, &asm, &tmp.join(format!("ours_{}.o", round)))
    });
    let system = best_cli_duration(2, |round| {
        run_system_as(&asm, &tmp.join(format!("ref_{}.o", round)))
    });

    assert!(
        ours <= Duration::from_secs(5),
        "afs-as CLI absolute throughput regressed: ours {:?}, system {:?}",
        ours,
        system
    );
}
