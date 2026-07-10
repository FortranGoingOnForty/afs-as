//! x14: seeded whole-file differential fuzz for the x86_64 pipeline.
//! Program-shaped cases drawn from the implemented dialect are
//! assembled by gas and by assemble_x86(); .text must be
//! byte-identical and relocs/symbols/sections policy-equal (the x13
//! normalize rules). A garbage companion feeds token soup through the
//! full pipeline: errors are fine, panics are not.

#[path = "common/elf.rs"]
mod celf;
#[path = "common/x86_gen.rs"]
mod x86_gen;

use afs_as::elf::{parse_elf, ELFOSABI_FREEBSD, ELFOSABI_NONE};
use afs_as::x86::assemble::assemble_x86;

const SEEDS: u64 = 96;
const GARBAGE_SEEDS: u64 = 512;

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

#[test]
fn seeded_cases_match_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_differential_fuzz",
            "seeded_cases_match_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_fuzz");
    let mut failures = Vec::new();
    for seed in 0..SEEDS {
        let src = x86_gen::generate_supported_case(seed);
        let src_path = tmp.path(&format!("_{}.s", seed));
        let obj_path = tmp.path(&format!("_{}.o", seed));
        std::fs::write(&src_path, &src).unwrap();
        let out = std::process::Command::new(&gas)
            .arg("--64")
            .arg("-o")
            .arg(&obj_path)
            .arg(&src_path)
            .output()
            .expect("run gas");
        if !out.status.success() {
            // gas rejecting a case means the generator (not the
            // assembler under test) is wrong — report with source.
            failures.push(format!(
                "seed {}: gas rejected the generated case:\n{}\n--- source ---\n{}",
                seed,
                String::from_utf8_lossy(&out.stderr),
                src
            ));
            continue;
        }
        let gas_obj = parse_elf(&std::fs::read(&obj_path).unwrap()).expect("lift gas");

        let ours = match assemble_x86(&src, host_osabi()) {
            Ok(o) => o,
            Err(e) => {
                failures.push(format!(
                    "seed {}: our assembler failed: {}\n{}",
                    seed, e, src
                ));
                continue;
            }
        };
        let gas_text = gas_obj
            .section_by_name(".text")
            .map(|s| celf::canonicalize_nop_fill(&s.data));
        let our_text = ours
            .section_by_name(".text")
            .map(|s| celf::canonicalize_nop_fill(&s.data));
        if gas_text != our_text {
            let (g, o) = (gas_text.unwrap_or_default(), our_text.unwrap_or_default());
            let first = g
                .iter()
                .zip(o.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(g.len().min(o.len()));
            failures.push(format!(
                "seed {}: .text diverges at byte {} (gas len {}, ours {})\n  gas:  {:02x?}\n  ours: {:02x?}",
                seed,
                first,
                g.len(),
                o.len(),
                &g[first.saturating_sub(8)..(first + 8).min(g.len())],
                &o[first.saturating_sub(8)..(first + 8).min(o.len())],
            ));
            continue;
        }
        let a = celf::normalize(&gas_obj);
        let b = celf::normalize(&ours);
        if a != b {
            failures.push(format!(
                "seed {}: policy divergence\n  gas relocs:  {:?}\n  our relocs:  {:?}\n  gas syms:  {:?}\n  our syms:  {:?}",
                seed, a.relocs, b.relocs, a.symbols, b.symbols
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} seeds diverge:\n\n{}",
        failures.len(),
        SEEDS,
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}

#[test]
fn garbage_cases_do_not_panic() {
    let mut errors = 0usize;
    for seed in 0..GARBAGE_SEEDS {
        let src = x86_gen::generate_garbage_case(seed);
        let outcome = std::panic::catch_unwind(|| assemble_x86(&src, ELFOSABI_NONE));
        match outcome {
            Ok(Ok(_)) => {} // rare but legal: soup that happens to parse
            Ok(Err(_)) => errors += 1,
            Err(_) => panic!("seed {} panicked on:\n{}", seed, src),
        }
    }
    // Nearly all soup must be rejected; if most of it assembles the
    // generator has gone soft.
    assert!(
        errors as u64 > GARBAGE_SEEDS * 9 / 10,
        "only {}/{} garbage cases errored",
        errors,
        GARBAGE_SEEDS
    );
}
