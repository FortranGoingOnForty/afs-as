use std::env;
use std::path::{Path, PathBuf};
use std::process;

const USAGE: &str = "\
afs-as: ARM64 assembler for macOS

usage: afs-as <input.s> [-o <output.o>]
       afs-as --help
       afs-as --version

Only a single input file is supported.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Assemble { input: PathBuf, output: PathBuf },
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
        Ok(Command::Assemble { input, output }) => {
            afs_as::assemble::assemble_file(&input, &output)
                .map_err(|err| (1, err.to_string()))
        }
        Err(message) => Err((2, format!("afs-as: {}\n\n{}", message, USAGE))),
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(Command::Help),
            "--version" | "-V" => return Ok(Command::Version),
            "-o" => {
                let Some(path) = args.next() else {
                    return Err("option '-o' requires an output path".into());
                };
                output = Some(PathBuf::from(path));
            }
            _ if arg.starts_with('-') => {
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

    let output = output.unwrap_or_else(|| default_output_path(&input));
    Ok(Command::Assemble { input, output })
}

fn default_output_path(input: &Path) -> PathBuf {
    input.with_extension("o")
}

#[cfg(test)]
mod tests {
    use super::{default_output_path, parse_args, Command};
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
            })
        );
    }

    #[test]
    fn parse_rejects_missing_input() {
        assert_eq!(parse(Vec::<String>::new()), Err("missing input file".into()));
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
    fn parse_rejects_unknown_option() {
        assert_eq!(
            parse(["--wat"]),
            Err("unrecognized option '--wat'".into())
        );
    }

    #[test]
    fn default_output_replaces_existing_extension() {
        assert_eq!(default_output_path(PathBuf::from("foo.s").as_path()), PathBuf::from("foo.o"));
        assert_eq!(default_output_path(PathBuf::from("foo").as_path()), PathBuf::from("foo.o"));
    }
}
