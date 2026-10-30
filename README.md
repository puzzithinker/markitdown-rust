# markitdown

Convert files to Markdown for LLM and text pipelines.

This is an independent Rust library and `markitdown` command. It follows the
idea of [microsoft/markitdown](https://github.com/microsoft/markitdown) v0.1.9.
It is not a Microsoft project, it is not affiliated with Microsoft, and it does
not use Microsoft logos or trademarks.

Office documents (DOCX, XLSX, PPTX, and the legacy DOC, XLS, and PPT formats)
are parsed with [undoc](https://docs.rs/undoc). PDFs are parsed with
[unpdf](https://docs.rs/unpdf). Markdown for those formats is whatever those
libraries emit.

## Build

Rust 1.89 or newer. The lockfile pins `hayro-jbig2` 0.3.0 because 0.3.1
asks for Rust 1.92.

```bash
cargo build --release
cargo test
```

The binary is `target/release/markitdown`.

## Command

```bash
markitdown report.docx
markitdown notes.csv -o notes.md
markitdown -x .csv < rows.csv
```

| Flag | Meaning |
| --- | --- |
| file | Path to convert. Omit it to read stdin. |
| `-o`, `--output` | Write Markdown to a file. Stdout is unchanged. |
| `-x`, `--extension` | Extension hint, such as `.csv`. |
| `-m`, `--mime-type` | Media type hint. It must contain one `/`. |
| `-c`, `--charset` | Text encoding, for example `iso-8859-1`. |
| `--keep-data-uris` | Leave `data:` URIs in HTML as they are. |
| `-v`, `--version` | Print the version. |
| `--list-plugins` | This build has no plugin loader. |

Stdout gets one trailing newline. `-o` writes the Markdown without adding one.
A broken pipe on stdout exits 0.

`--use-docintel` / `-d`, `--use-cu` / `--use-content-understanding`, and the
Azure endpoint and analyzer flags are accepted and then exit 1. Azure Document
Intelligence and Azure Content Understanding are not included.

Exit 0 on success. Exit 1 on a conversion or usage error.

## Library

```rust
use markitdown::{MarkItDown, Options, StreamInfo};

let result = MarkItDown::new()
    .convert_bytes(b"name,city\nAda,London\n", StreamInfo::new().with_extension("csv"))
    .unwrap();
assert!(result.markdown.contains("Ada"));

let _from_disk = MarkItDown::new().convert_path("report.pdf");
let _page = MarkItDown::new().convert_uri("file:///tmp/notes.md");

let _limited = MarkItDown::with_options(Options {
    keep_data_uris: true,
    max_zip_depth: 2,
    ..Options::default()
});
```

`convert_path_with_info` applies extension, media type, and charset hints and
still gives PDF and Office parsers the real path.

Register another format with `MarkItDown::register`. Lower `priority` runs
first. `PRIORITY_SPECIFIC` is `0.0` and `PRIORITY_GENERIC` is `10.0`. Within
one priority, the most recently registered converter runs first. A converter
that accepts an input and then fails is recorded. If every attempt fails, the
error is `Error::FileConversion`. If nobody accepts the input, the error is
`Error::UnsupportedFormat`.

After a successful conversion, trailing whitespace is stripped from each line
and runs of three or more newlines collapse to two.

## Formats

| Input | How |
| --- | --- |
| PDF | unpdf |
| DOCX, XLSX, PPTX, DOC, XLS, PPT | undoc, including legacy OLE files |
| HTML, XHTML | htmd, skipping `script`, `style`, and `noscript`. A `<title>` becomes `title`. |
| Wikipedia | When the URL host is `wikipedia.org` or a language subdomain, keep `#mw-content-text` and drop the TOC, reference lists, and navboxes, then run the HTML converter. A saved page with no Wikipedia URL stays generic HTML. |
| CSV | Markdown table. The first row is the header. Short rows are padded. Pipes are escaped. Newlines inside cells become spaces. Blank rows around the table are dropped. |
| Plain text | `.txt`, `.text`, `.md`, `.markdown`, `.json`, `.jsonl`, `.xml`, and `text/*`, `application/json`, `application/markdown`, `application/xml`. JSON is copied through, not pretty-printed. |
| RSS, Atom | feed-rs. Item HTML is converted. XML that is not a feed falls through to plain text. |
| EPUB | Spine order. Title and creator come from the OPF. |
| Jupyter | Markdown cells as they are, code in a fenced block, raw cells in a plain fence. |
| ZIP | Each member is converted on its own. Directories, encrypted entries, unsupported members, and `..` names are skipped. Limits: 10,000 entries, 512 MiB uncompressed, nesting depth 3. |
| Outlook `.msg` | Subject, From, To, Cc, date, plain body (or HTML when the plain body is empty), and attachments. |
| JPEG, PNG, GIF, WebP, BMP, TIFF | Format, pixel size, and EXIF. No image captioning. |
| MP3, WAV, FLAC, M4A | Container, duration, and title, artist, and album tags. No transcript. |

Text, CSV, Jupyter, and ZIP are always compiled. The other formats are feature
flags, all on by default: `pdf`, `office`, `html`, `rss`, `epub`, `outlook`,
`image`, `audio`, `url`, `cli`.

`file:` and `data:` URIs work without the network. `http:` and `https:` need
the `url` feature.

## What this port leaves out

- Azure Document Intelligence and Azure Content Understanding
- Speech to text and image captions
- YouTube transcripts
- Bing search-result pages
- A bundled LLM client
- Python plugin loading, MCP, and Docker

Extension is a Rust `Converter` trait.

## Detection

Hints from the caller win when they name a specific type. Otherwise the path
or `Content-Disposition` filename is used, then the bytes. PDF, OLE, OOXML,
and EPUB are recognized before a generic ZIP, so a `.docx` is not unpacked as
an archive. A generic extension (`.zip`, `.bin`, `.dat`, or none) gives way
to a specific sniff. A specific extension stays first, and a different sniff
is tried second.

Charset hints are honored. Otherwise the encoding is guessed with `chardetng`
and decoded with `encoding_rs`. A leading UTF-8 BOM is removed.

## Security

Fetched `http` and `https` URLs are refused when the scheme is anything else,
or when the host is `localhost`, `*.localhost`, or `metadata.google.internal`.
Names are resolved, and every address is checked, including IPv4-mapped IPv6.
Loopback, link-local (which covers `169.254.169.254`), private, unspecified,
multicast, broadcast, and carrier-grade NAT (`100.64.0.0/10`) addresses are
refused. The same check runs on each redirect. The client allows about five
redirects, waits 30 seconds, and stops reading after 64 MiB.

The check and the HTTP client resolve names separately, so a resolver that
changes its answer between the two lookups can still defeat the filter. Do
not point this at untrusted networks without your own network policy.

Paths are not sandboxed. `convert_path` and `file:` URIs use the process's
normal filesystem access, the same way the Python tool does.

ZIP members whose names escape the archive are skipped. Nested archives stop
at `Options::max_zip_depth`.

## License

MIT. See [LICENSE](LICENSE).
