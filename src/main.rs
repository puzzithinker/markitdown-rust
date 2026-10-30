use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Parser};

use markitdown::{Error, MarkItDown, Options, StreamInfo};

#[derive(Parser)]
#[command(
    name = "markitdown",
    version,
    about = "Convert files to Markdown for LLM and text pipelines",
    disable_version_flag = true
)]
struct Cli {
    /// File to convert. Reads stdin when omitted.
    file: Option<PathBuf>,

    /// Write Markdown to this path instead of stdout.
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

    /// Hint the file extension, for example `.csv` or `csv`.
    #[arg(short = 'x', long = "extension")]
    extension: Option<String>,

    /// Hint the media type, for example `text/csv`.
    #[arg(short = 'm', long = "mime-type")]
    mime_type: Option<String>,

    /// Decode text with this charset instead of guessing.
    #[arg(short = 'c', long = "charset")]
    charset: Option<String>,

    /// Leave `data:` URIs in HTML intact.
    #[arg(long = "keep-data-uris")]
    keep_data_uris: bool,

    /// Print version.
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    version: (),

    /// Report that this build has no plugin loader.
    #[arg(long = "list-plugins")]
    list_plugins: bool,

    /// Not included in this port.
    #[arg(short = 'd', long = "use-docintel")]
    use_docintel: bool,

    /// Not included in this port.
    #[arg(long = "use-cu", visible_alias = "use-content-understanding")]
    use_cu: bool,

    /// Not included in this port.
    #[arg(long = "endpoint", hide = true)]
    endpoint: Option<String>,

    /// Not included in this port.
    #[arg(long = "docintel-endpoint", hide = true)]
    docintel_endpoint: Option<String>,

    /// Not included in this port.
    #[arg(long = "docintel-key", hide = true)]
    docintel_key: Option<String>,

    /// Not included in this port.
    #[arg(long = "cu-endpoint", hide = true)]
    cu_endpoint: Option<String>,

    /// Not included in this port.
    #[arg(long = "cu-analyzer", hide = true)]
    cu_analyzer: Option<String>,

    /// Not included in this port.
    #[arg(long = "cu-file-types", hide = true)]
    cu_file_types: Option<String>,
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let code = match err.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
                _ => 1,
            };
            let _ = err.print();
            return ExitCode::from(code);
        }
    };

    if azure_requested(&cli) {
        eprintln!(
            "Azure Document Intelligence and Azure Content Understanding are not included in this port."
        );
        return ExitCode::from(1);
    }
    if cli.list_plugins {
        println!("No plugin loader is included. Register converters from Rust with MarkItDown::register.");
        return ExitCode::SUCCESS;
    }

    let engine = MarkItDown::with_options(Options {
        keep_data_uris: cli.keep_data_uris,
        ..Options::default()
    });

    let converted = if let Some(path) = &cli.file {
        let mut info = StreamInfo::from_path(path);
        if apply_hints(&mut info, &cli).is_err() {
            return ExitCode::from(1);
        }
        engine.convert_path_with_info(path, info)
    } else {
        let mut bytes = Vec::new();
        if let Err(err) = io::stdin().read_to_end(&mut bytes) {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
        let mut info = StreamInfo::new();
        if apply_hints(&mut info, &cli).is_err() {
            return ExitCode::from(1);
        }
        engine.convert_bytes(&bytes, info)
    };

    match converted {
        Ok(result) => write_markdown(cli.output.as_ref(), &result.markdown),
        Err(err) => {
            eprint_error(&err);
            ExitCode::from(1)
        }
    }
}

fn azure_requested(cli: &Cli) -> bool {
    cli.use_docintel
        || cli.use_cu
        || cli.endpoint.is_some()
        || cli.docintel_endpoint.is_some()
        || cli.docintel_key.is_some()
        || cli.cu_endpoint.is_some()
        || cli.cu_analyzer.is_some()
        || cli.cu_file_types.is_some()
}

fn apply_hints(info: &mut StreamInfo, cli: &Cli) -> Result<(), ()> {
    if let Some(extension) = &cli.extension {
        let normalized = normalize_cli_extension(extension).ok_or(())?;
        info.extension = Some(normalized);
    }
    if let Some(mime) = &cli.mime_type {
        let mime = mime.trim();
        if !mime.contains('/') {
            eprintln!("mime type must contain one slash, for example text/csv");
            return Err(());
        }
        info.mimetype = Some(mime.to_string());
    }
    if let Some(charset) = &cli.charset {
        let charset = charset.trim();
        if encoding_rs::Encoding::for_label(charset.as_bytes()).is_none() {
            eprintln!("unknown charset {charset}");
            return Err(());
        }
        info.charset = Some(charset.to_string());
    }
    Ok(())
}

fn normalize_cli_extension(extension: &str) -> Option<String> {
    let trimmed = extension.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed.contains('/') || trimmed.contains('\\') {
        eprintln!("extension must be a file suffix such as .csv");
        return None;
    }
    let normalized = if trimmed.starts_with('.') {
        trimmed.to_ascii_lowercase()
    } else {
        format!(".{}", trimmed.to_ascii_lowercase())
    };
    if normalized == "." {
        eprintln!("extension must be a file suffix such as .csv");
        return None;
    }
    Some(normalized)
}

fn write_markdown(path: Option<&PathBuf>, markdown: &str) -> ExitCode {
    match path {
        Some(path) => match std::fs::write(path, markdown) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(1)
            }
        },
        None => {
            let mut stdout = io::stdout().lock();
            match writeln!(stdout, "{markdown}") {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("{err}");
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn eprint_error(err: &Error) {
    match err {
        Error::FileConversion(_) => eprint!("{err}"),
        other => eprintln!("{other}"),
    }
}
