//! Assumes GitHub release ZIPs resolved by fetch.ps1 and Yomitan format 3.

use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

// A source's sequence IDs must stay together and never match another source.
const SEQUENCE_STRIDE: i64 = 1_000_000_000;
const MAX_BANK_BYTES: usize = 8 * 1024 * 1024;
const MAX_BANK_ROWS: usize = 5_000;
const RELEASES: &str = "https://github.com/caocaochan/Megadict/releases";

#[derive(Deserialize, Serialize)]
struct Source {
    id: String,
    group: String,
    release: String,
    asset: String,
    sha256: String,
    path: PathBuf,
}

#[derive(Serialize)]
struct SourceReport<'a> {
    id: &'a str,
    title: &'a str,
    revision: &'a str,
    release: &'a str,
    asset: &'a str,
    sha256: &'a str,
    rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    attribution: Option<&'a str>,
}

struct Archive<W: Write + Seek> {
    zip: ZipWriter<W>,
    bank: Vec<u8>,
    row: Vec<u8>,
    bank_rows: usize,
    bank_count: usize,
    rows: usize,
}

impl<W: Write + Seek> Archive<W> {
    fn new(writer: W) -> Self {
        Self {
            zip: ZipWriter::new(writer),
            bank: vec![b'['],
            row: Vec::new(),
            bank_rows: 0,
            bank_count: 0,
            rows: 0,
        }
    }

    fn write_file(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.zip.start_file(
            name,
            SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .compression_level(Some(5)),
        )?;
        self.zip.write_all(bytes)?;
        Ok(())
    }

    fn write_row(&mut self, row: &[Value]) -> Result<()> {
        self.row.clear();
        serde_json::to_writer(&mut self.row, row)?;
        if self.bank_rows == MAX_BANK_ROWS
            || (self.bank_rows > 0 && self.bank.len() + self.row.len() + 2 > MAX_BANK_BYTES)
        {
            self.flush_bank()?;
        }
        if self.bank_rows > 0 {
            self.bank.push(b',');
        }
        self.bank.extend_from_slice(&self.row);
        self.bank_rows += 1;
        self.rows += 1;
        Ok(())
    }

    fn flush_bank(&mut self) -> Result<()> {
        if self.bank_rows == 0 {
            return Ok(());
        }
        self.bank_count += 1;
        let mut bank = std::mem::take(&mut self.bank);
        bank.push(b']');
        self.write_file(&format!("term_bank_{}.json", self.bank_count), &bank)?;
        bank.clear();
        bank.push(b'[');
        self.bank = bank;
        self.bank_rows = 0;
        Ok(())
    }
}

fn main() -> Result<()> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    let [manifest, output, revision] = arguments.as_slice() else {
        bail!("Usage: megadict <resolved.json> <output.zip> <revision>");
    };
    let revision = revision.to_str().context("Revision must be UTF-8")?;
    ensure!(
        revision.split('.').count() == 4
            && revision
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        "Revision must be four dot-separated integers",
    );
    let manifest = Path::new(manifest).canonicalize()?;
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .context("Expected the manifest inside a cache directory")?;
    let sources: Vec<Source> = serde_json::from_reader(File::open(&manifest)?)?;
    let output = Path::new(output);
    let output_dir = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let output_dir = output_dir.unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_dir)?;
    let temporary = NamedTempFile::new_in(output_dir)?;
    let mut archive = Archive::new(BufWriter::new(temporary.as_file().try_clone()?));
    let mut styles = String::from(
        "[data-sc-megadict-label] { display: block; font-size: .8em; font-weight: 600; opacity: .75; margin-bottom: .25em; }\n",
    );
    let mut reports = Vec::new();
    let mut ids = HashSet::new();
    let mut attributions = Vec::new();

    for (position, source) in sources
        .iter()
        .filter(|s| s.group == "dictionaries")
        .enumerate()
    {
        let id = &source.id;
        ensure!(
            !id.is_empty()
                && id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
            "Invalid source ID {id:?}",
        );
        ensure!(ids.insert(id), "Duplicate source ID {id}");
        let path = root.join(&source.path);
        let mut input =
            ZipArchive::new(File::open(&path).with_context(|| path.display().to_string())?)?;
        let before = archive.rows;
        let index = import_yomitan(&mut archive, &mut input, source, position, &mut styles)
            .with_context(|| format!("Importing {} ({})", source.asset, source.release))?;
        let rows = archive.rows - before;
        ensure!(
            rows > 0,
            "{} did not contribute any definitions",
            source.asset
        );
        let title = index_string(&index, "title")?.to_owned();
        let revision = index_string(&index, "revision")?.to_owned();
        let attribution = index
            .get("attribution")
            .and_then(Value::as_str)
            .map(str::to_owned);
        attributions.push(format!(
            "{title} ({revision})\n{}{}",
            source.release,
            attribution
                .as_ref()
                .map(|text| format!("\n{text}"))
                .unwrap_or_default(),
        ));
        archive.write_file(
            &format!("sources/{id}/index.json"),
            &serde_json::to_vec_pretty(&index)?,
        )?;
        archive.flush_bank()?;
        eprintln!("{title} ({revision}): {rows} term rows");
        reports.push((source, title, revision, rows, attribution));
    }

    ensure!(
        !reports.is_empty(),
        "No definition archives in the manifest"
    );
    let report: Vec<_> = reports
        .iter()
        .map(
            |(source, title, revision, rows, attribution)| SourceReport {
                id: &source.id,
                title,
                revision,
                release: &source.release,
                asset: &source.asset,
                sha256: &source.sha256,
                rows: *rows,
                attribution: attribution.as_deref(),
            },
        )
        .collect();
    let index = json!({
        "title": "Megadict",
        "revision": revision,
        "format": 3,
        "sequenced": true,
        "author": "caocaochan and the original dictionary contributors",
        "description": "Chinese dictionary collection with source labels for each definition.",
        "sourceLanguage": "zh",
        "attribution": attributions.join("\n\n"),
        "isUpdatable": true,
        "indexUrl": format!("{RELEASES}/latest/download/index.json"),
        "downloadUrl": format!("{RELEASES}/download/v{revision}/Megadict.zip"),
    });
    archive.write_file("index.json", &serde_json::to_vec_pretty(&index)?)?;
    archive.write_file("styles.css", styles.as_bytes())?;
    archive.write_file("sources.json", &serde_json::to_vec_pretty(&report)?)?;
    let rows = archive.rows;
    let bank_count = archive.bank_count;
    archive.zip.finish()?.flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(output)?;
    eprintln!(
        "{}: {} sources, {rows} term rows, {bank_count} banks",
        output.display(),
        reports.len()
    );
    Ok(())
}

fn index_string<'a>(index: &'a Value, field: &str) -> Result<&'a str> {
    index
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .with_context(|| format!("Missing dictionary index field {field}"))
}

fn read_member(input: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input.by_name(name)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn import_yomitan<W: Write + Seek>(
    output: &mut Archive<W>,
    input: &mut ZipArchive<File>,
    source: &Source,
    position: usize,
    styles: &mut String,
) -> Result<Value> {
    let index: Value = serde_json::from_slice(&read_member(input, "index.json")?)?;
    let title = index_string(&index, "title")?;
    let format = index.get("format").or_else(|| index.get("version"));
    ensure!(
        format.and_then(Value::as_u64) == Some(3),
        "Expected a Yomitan format 3 archive"
    );
    ensure!(
        index
            .get("tagMeta")
            .is_none_or(|tags| tags.as_object().is_some_and(Map::is_empty)),
        "Legacy index tags need handling"
    );

    let mut banks = Vec::new();
    let mut assets = HashSet::new();
    for name in input.file_names() {
        let name = name?;
        if name == "index.json" || name == "styles.css" || name.ends_with('/') {
            continue;
        }
        if let Some(number) = name
            .strip_prefix("term_bank_")
            .and_then(|s| s.strip_suffix(".json"))
        {
            banks.push((number.parse::<usize>()?, name.into_owned()));
        } else {
            ensure!(
                name.contains('/') || !name.contains("_bank_"),
                "Unsupported dictionary bank {name}"
            );
            assets.insert(name.into_owned());
        }
    }
    banks.sort_unstable();
    let mut asset_names: Vec<_> = assets.iter().collect();
    asset_names.sort_unstable();
    for name in asset_names {
        let member = input.by_name(name)?;
        ensure!(
            member.enclosed_name().is_some(),
            "Invalid archive member path {name:?}"
        );
        output
            .zip
            .raw_copy_file_rename(member, format!("sources/{}/{name}", source.id))?;
    }

    if input.index_for_name("styles.css").is_some() {
        let css = String::from_utf8(read_member(input, "styles.css")?)?;
        // The source styles are ordinary selectors and intentionally contain no imports.
        // Native CSS nesting scopes them to their own glossary content.
        ensure!(
            !css.contains("@import") && !css.contains("url("),
            "Source CSS references external assets"
        );
        styles.push_str(&format!(
            "[data-sc-megadict=\"{}\"] {{\n{css}\n}}\n",
            source.id
        ));
    }

    let scope = GlossaryScope {
        id: &source.id,
        title,
        assets: &assets,
    };
    let mut bytes = Vec::new();
    for (offset, (number, name)) in banks.into_iter().enumerate() {
        ensure!(
            number == offset + 1,
            "Missing or duplicate term bank near {name}"
        );
        bytes.clear();
        input.by_name(&name)?.read_to_end(&mut bytes)?;
        let rows: Vec<Vec<Value>> = serde_json::from_slice(&bytes).with_context(|| name.clone())?;
        for mut row in rows {
            ensure!(row.len() == 8, "Invalid term row in {name}");
            let sequence = row[6].as_i64().context("Invalid term sequence")?;
            if sequence >= 0 {
                ensure!(
                    sequence < SEQUENCE_STRIDE,
                    "Sequence out of range in {name}: {sequence}"
                );
                row[6] = json!(position as i64 * SEQUENCE_STRIDE + sequence);
            }
            scope.label(row[5].as_array_mut().context("Invalid term glossary")?)?;
            output.write_row(&row)?;
        }
    }
    Ok(index)
}

struct GlossaryScope<'a> {
    id: &'a str,
    title: &'a str,
    assets: &'a HashSet<String>,
}

impl GlossaryScope<'_> {
    fn label(&self, glosses: &mut Vec<Value>) -> Result<()> {
        let label =
            json!({"tag": "span", "data": {"megadict-label": self.id}, "content": self.title});
        let mut pending_label = Some(label);
        for gloss in glosses.iter_mut() {
            if gloss.is_array() {
                // Yomitan's dictionary-defined deinflection is a separate glossary type.
                continue;
            }
            let value = std::mem::take(gloss);
            let mut content = match value {
                Value::String(_) => value,
                Value::Object(mut object) => match object.get("type").and_then(Value::as_str) {
                    Some("structured-content") => object
                        .remove("content")
                        .context("Missing structured content")?,
                    Some("text") => object.remove("text").context("Missing text content")?,
                    Some("image") => {
                        object.remove("type");
                        object.insert("tag".into(), json!("img"));
                        Value::Object(object)
                    }
                    _ => bail!("Unsupported glossary object in {}", self.id),
                },
                _ => bail!("Unsupported glossary value in {}", self.id),
            };
            self.rewrite_media(&mut content)?;
            if let Some(label) = pending_label.take() {
                content = Value::Array(vec![label, content]);
            }
            *gloss = json!({ "type": "structured-content", "content": {
                "tag": "div", "data": {"megadict": self.id}, "content": content,
            }});
        }
        if let Some(label) = pending_label {
            glosses.push(json!({"type": "structured-content", "content": label}));
        }
        Ok(())
    }

    fn rewrite_media(&self, content: &mut Value) -> Result<()> {
        match content {
            Value::Array(children) => {
                for child in children {
                    self.rewrite_media(child)?;
                }
            }
            Value::Object(node) => {
                if node.get("tag").and_then(Value::as_str) == Some("img") {
                    let path = node.get_mut("path").context("Missing image path")?;
                    let original = path.as_str().context("Invalid image path")?;
                    ensure!(
                        self.assets.contains(original),
                        "Missing {} image {original}",
                        self.id
                    );
                    *path = json!(format!("sources/{}/{original}", self.id));
                }
                if let Some(child) = node.get_mut("content") {
                    self.rewrite_media(child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
