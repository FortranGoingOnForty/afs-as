use std::env;
use std::fs;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

const USAGE: &str = "\
afs-as: assembler (ARM64 Mach-O, x86_64 ELF)

usage: afs-as <input.s> [-o <output.o>]
       afs-as --64 <input.s> [-o <output.o>]
       afs-as - -o <output.o>
       afs-as - -o -
       afs-as --help
       afs-as --version

options:
  -o <path>    write object to path, or '-' for stdout
  --64         assemble x86_64 AT&T source to an ELF64 object
  --           stop option parsing

exit status:
  0            success
  1            parse or assembly failure
  2            command-line usage error

Only a single input file is supported.
Input '-' requires an explicit -o <output.o> or -o -.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Arm64Macho,
    X8664Elf,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Assemble {
        input: PathBuf,
        output: PathBuf,
        target: Target,
    },
    Help,
    Version,
}

fn main() {
    match run() {
        Ok(()) => {}
        Err((code, message)) => {
            if !message.is_empty() {
                eprintln!("{}", message);
            }
            process::exit(code);
        }
    }
}

fn run() -> Result<(), (i32, String)> {
    match parse_args(env::args().skip(1)) {
        Ok(Command::Help) => {
            println!("{}", USAGE);
            Ok(())
        }
        Ok(Command::Version) => {
            println!("afs-as {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Ok(Command::Assemble {
            input,
            output,
            target,
        }) => match target {
            Target::Arm64Macho => assemble_cli(&input, &output).map_err(|err| (1, err.to_string())),
            Target::X8664Elf => assemble_cli_x86(&input, &output).map_err(|err| (1, err)),
        },
        Err(message) => Err((2, format!("afs-as: {}\n\n{}", message, USAGE))),
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut target = Target::Arm64Macho;
    let mut parsing_options = true;

    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" if parsing_options => {
                parsing_options = false;
            }
            "--help" | "-h" if parsing_options => return Ok(Command::Help),
            "--version" | "-V" if parsing_options => return Ok(Command::Version),
            "--64" if parsing_options => target = Target::X8664Elf,
            "-o" if parsing_options => {
                let Some(path) = args.next() else {
                    return Err("option '-o' requires an output path".into());
                };
                output = Some(PathBuf::from(path));
            }
            "-" => {
                if input.is_some() {
                    return Err("multiple input files are not supported (extra input '-')".into());
                }
                input = Some(PathBuf::from(arg));
            }
            _ if parsing_options && arg.starts_with('-') => {
                return Err(format!("unrecognized option '{}'", arg));
            }
            _ => {
                if input.is_some() {
                    return Err(format!(
                        "multiple input files are not supported (extra input '{}')",
                        arg
                    ));
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }

    let Some(input) = input else {
        return Err("missing input file".into());
    };

    let output = match output {
        Some(output) => output,
        None if is_stdio_path(&input) => {
            return Err("input '-' requires explicit -o <output.o> or -o -".into());
        }
        None => default_output_path(&input),
    };
    Ok(Command::Assemble {
        input,
        output,
        target,
    })
}

fn default_output_path(input: &Path) -> PathBuf {
    input.with_extension("o")
}

fn is_stdio_path(path: &Path) -> bool {
    path == Path::new("-")
}

/// The `--64` path: x86_64 AT&T source to an ELF64 relocatable
/// object. Matches the driver's `<as> --64 -o obj.o asm.s` contract.
fn assemble_cli_x86(input: &Path, output: &Path) -> Result<(), String> {
    let stdin_display = Path::new("<stdin>");
    let input_display = if is_stdio_path(input) {
        stdin_display
    } else {
        input
    };
    let read_err = |e: io::Error| format!("{}: {}", input_display.display(), e);
    let src = if is_stdio_path(input) {
        let mut src = String::new();
        io::stdin().read_to_string(&mut src).map_err(read_err)?;
        src
    } else {
        fs::read_to_string(input).map_err(read_err)?
    };

    let osabi = if cfg!(target_os = "freebsd") {
        afs_as::elf::ELFOSABI_FREEBSD
    } else {
        afs_as::elf::ELFOSABI_NONE
    };
    let obj = afs_as::x86::assemble::assemble_x86(&src, osabi)
        .map_err(|e| e.with_source_context(input_display, &src).to_string())?;
    let bytes =
        afs_as::elf::write_elf(&obj).map_err(|e| format!("{}: {}", input_display.display(), e))?;

    let write_err = |e: io::Error| format!("{}: {}", output.display(), e);
    if is_stdio_path(output) {
        let stdout = io::stdout();
        let mut w = stdout.lock();
        w.write_all(&bytes).map_err(write_err)?;
        w.flush().map_err(write_err)?;
    } else {
        fs::write(output, &bytes).map_err(write_err)?;
    }
    Ok(())
}

fn assemble_cli(input: &Path, output: &Path) -> Result<(), afs_as::assemble::AsmError> {
    let stdin_display = Path::new("<stdin>");
    let stdout_display = Path::new("<stdout>");
    let input_display = if is_stdio_path(input) {
        stdin_display
    } else {
        input
    };

    let src = if is_stdio_path(input) {
        let mut src = String::new();
        io::stdin().read_to_string(&mut src).map_err(|err| {
            afs_as::assemble::AsmError::new(format!("{}", err)).with_path(input_display)
        })?;
        src
    } else {
        fs::read_to_string(input)
            .map_err(|err| afs_as::assemble::AsmError::new(format!("{}", err)).with_path(input))?
    };

    let obj = afs_as::assemble::assemble_source(&src)
        .map_err(|err| err.with_source_context(input_display, &src))?;

    if is_stdio_path(output) {
        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        afs_as::macho::write_macho(&obj, &mut writer).map_err(|err| {
            afs_as::assemble::AsmError::new(format!("writing output: {}", err))
                .with_path(stdout_display)
        })?;
        writer.flush().map_err(|err| {
            afs_as::assemble::AsmError::new(format!("writing output: {}", err))
                .with_path(stdout_display)
        })?;
    } else {
        let file = fs::File::create(output)
            .map_err(|err| afs_as::assemble::AsmError::new(format!("{}", err)).with_path(output))?;
        let mut writer = BufWriter::new(file);
        afs_as::macho::write_macho(&obj, &mut writer).map_err(|err| {
            afs_as::assemble::AsmError::new(format!("writing output: {}", err)).with_path(output)
        })?;
        writer.flush().map_err(|err| {
            afs_as::assemble::AsmError::new(format!("writing output: {}", err)).with_path(output)
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{default_output_path, parse_args, Command, Target};
    use std::path::PathBuf;

    fn parse<I, S>(args: I) -> Result<Command, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        parse_args(args.into_iter().map(Into::into))
    }

    #[test]
    fn parse_help_flag() {
        assert_eq!(parse(["--help"]), Ok(Command::Help));
        assert_eq!(parse(["-h"]), Ok(Command::Help));
    }

    #[test]
    fn parse_version_flag() {
        assert_eq!(parse(["--version"]), Ok(Command::Version));
        assert_eq!(parse(["-V"]), Ok(Command::Version));
    }

    #[test]
    fn parse_input_with_explicit_output() {
        assert_eq!(
            parse(["input.s", "-o", "output.o"]),
            Ok(Command::Assemble {
                input: PathBuf::from("input.s"),
                output: PathBuf::from("output.o"),
                target: Target::Arm64Macho,
            })
        );
    }

    #[test]
    fn parse_input_uses_default_output() {
        assert_eq!(
            parse(["src/hello.s"]),
            Ok(Command::Assemble {
                input: PathBuf::from("src/hello.s"),
                output: PathBuf::from("src/hello.o"),
                target: Target::Arm64Macho,
            })
        );
    }

    #[test]
    fn parse_rejects_missing_input() {
        assert_eq!(
            parse(Vec::<String>::new()),
            Err("missing input file".into())
        );
    }

    #[test]
    fn parse_rejects_missing_output_path() {
        assert_eq!(
            parse(["input.s", "-o"]),
            Err("option '-o' requires an output path".into())
        );
    }

    #[test]
    fn parse_rejects_multiple_inputs() {
        assert_eq!(
            parse(["a.s", "b.s"]),
            Err("multiple input files are not supported (extra input 'b.s')".into())
        );
    }

    #[test]
    fn parse_stdin_requires_explicit_output() {
        assert_eq!(
            parse(["-"]),
            Err("input '-' requires explicit -o <output.o> or -o -".into())
        );
    }

    #[test]
    fn parse_accepts_stdin_and_stdout_paths() {
        assert_eq!(
            parse(["-", "-o", "-"]),
            Ok(Command::Assemble {
                input: PathBuf::from("-"),
                output: PathBuf::from("-"),
                target: Target::Arm64Macho,
            })
        );
    }

    #[test]
    fn parse_double_dash_stops_option_parsing() {
        assert_eq!(
            parse(["--", "--version.s"]),
            Ok(Command::Assemble {
                input: PathBuf::from("--version.s"),
                output: PathBuf::from("--version.o"),
                target: Target::Arm64Macho,
            })
        );
    }

    #[test]
    fn parse_rejects_unknown_option() {
        assert_eq!(parse(["--wat"]), Err("unrecognized option '--wat'".into()));
    }

    #[test]
    fn parse_64_flag_selects_elf_target() {
        assert_eq!(
            parse(["--64", "-o", "out.o", "in.s"]),
            Ok(Command::Assemble {
                input: PathBuf::from("in.s"),
                output: PathBuf::from("out.o"),
                target: Target::X8664Elf,
            })
        );
        // Flag order must not matter (the driver puts it first).
        assert_eq!(
            parse(["in.s", "--64"]),
            Ok(Command::Assemble {
                input: PathBuf::from("in.s"),
                output: PathBuf::from("in.o"),
                target: Target::X8664Elf,
            })
        );
    }

    #[test]
    fn default_output_replaces_existing_extension() {
        assert_eq!(
            default_output_path(PathBuf::from("foo.s").as_path()),
            PathBuf::from("foo.o")
        );
        assert_eq!(
            default_output_path(PathBuf::from("foo").as_path()),
            PathBuf::from("foo.o")
        );
    }
}
