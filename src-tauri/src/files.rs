use std::{
    io::{Cursor, Read, Write},
    sync::Arc,
};

use quick_xml::{
    escape::unescape,
    events::{BytesText, Event},
    Reader, Writer,
};
use serde_json::Value;
use thiserror::Error;
use tokio::task::JoinSet;
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

use crate::{
    models::{FileOutputMode, FileProgress, TranslateRequest},
    scheduler::{AiScheduler, Priority},
    store::AppStore,
};

#[derive(Debug, Error)]
pub enum FileError {
    #[error("{0}")]
    Message(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid ZIP/Office document: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type ProgressCallback = Arc<dyn Fn(FileProgress) + Send + Sync>;

#[derive(Debug)]
pub struct FileTranslationResult {
    pub filename: String,
    pub media_type: String,
    pub content: Vec<u8>,
    pub translated_segments: usize,
    pub skipped_segments: usize,
    pub total_segments: usize,
    pub total_batches: usize,
}

#[derive(Default)]
struct Counts {
    translated: usize,
    skipped: usize,
    total_segments: usize,
    total_batches: usize,
    completed_batches: usize,
    stage: String,
}

pub async fn translate_file(
    store: &AppStore,
    scheduler: &AiScheduler,
    filename: &str,
    bytes: Vec<u8>,
    options: TranslateRequest,
    output_mode: FileOutputMode,
    progress: ProgressCallback,
) -> Result<FileTranslationResult, FileError> {
    let extension = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut counts = Counts {
        stage: "preparing".to_string(),
        ..Counts::default()
    };
    report(&counts, &progress);
    let translated = match extension.as_str() {
        "docx" => {
            translate_office(
                store,
                scheduler,
                &bytes,
                &options,
                output_mode,
                OfficeKind::Word,
                &mut counts,
                &progress,
            )
            .await?
        }
        "pptx" => {
            translate_office(
                store,
                scheduler,
                &bytes,
                &options,
                output_mode,
                OfficeKind::PowerPoint,
                &mut counts,
                &progress,
            )
            .await?
        }
        "xlsx" => {
            translate_office(
                store,
                scheduler,
                &bytes,
                &options,
                output_mode,
                OfficeKind::Excel,
                &mut counts,
                &progress,
            )
            .await?
        }
        "json" => {
            translate_json(
                store,
                scheduler,
                &bytes,
                &options,
                output_mode,
                &mut counts,
                &progress,
            )
            .await?
        }
        "csv" => {
            translate_csv(
                store,
                scheduler,
                &bytes,
                &options,
                output_mode,
                &mut counts,
                &progress,
            )
            .await?
        }
        "srt" | "vtt" => {
            translate_caption(
                store,
                scheduler,
                &bytes,
                &options,
                output_mode,
                &mut counts,
                &progress,
            )
            .await?
        }
        "txt" | "md" | "markdown" | "html" | "htm" => {
            translate_plain_file(
                store,
                scheduler,
                &bytes,
                &options,
                output_mode,
                &mut counts,
                &progress,
            )
            .await?
        }
        "png" | "jpg" | "jpeg" | "webp" => {
            translate_image_file(
                store,
                scheduler,
                filename,
                bytes,
                &options,
                &mut counts,
                &progress,
            )
            .await?
        }
        _ => {
            return Err(FileError::Message(
                "Unsupported file type. Use DOCX, PPTX, XLSX, TXT, Markdown, HTML, CSV, JSON, SRT, VTT, PNG, JPG or WebP."
                    .to_string(),
            ))
        }
    };
    counts.stage = "completed".to_string();
    report(&counts, &progress);
    Ok(FileTranslationResult {
        filename: translated_filename(filename),
        media_type: mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string(),
        content: translated,
        translated_segments: counts.translated,
        skipped_segments: counts.skipped,
        total_segments: counts.total_segments,
        total_batches: counts.total_batches,
    })
}

fn report(counts: &Counts, progress: &ProgressCallback) {
    progress(FileProgress {
        stage: counts.stage.clone(),
        total_segments: counts.total_segments,
        translated_segments: counts.translated,
        skipped_segments: counts.skipped,
        total_batches: counts.total_batches,
        completed_batches: counts.completed_batches,
    });
}

async fn translate_image_file(
    _store: &AppStore,
    scheduler: &AiScheduler,
    filename: &str,
    bytes: Vec<u8>,
    options: &TranslateRequest,
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<u8>, FileError> {
    counts.stage = "translating".to_string();
    counts.total_segments += 1;
    counts.total_batches += 1;
    report(counts, progress);
    let translated = scheduler
        .translate_image(options.clone(), filename.to_string(), bytes, Priority::Low)
        .await
        .map_err(|error| FileError::Message(error.to_string()))?;
    counts.translated += 1;
    counts.completed_batches += 1;
    report(counts, progress);
    Ok(translated)
}

async fn translate_plain_file(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<u8>, FileError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FileError::Message("Only UTF-8 text files are supported".to_string()))?;
    Ok(translate_lines(
        store,
        scheduler,
        text,
        options,
        output_mode,
        counts,
        progress,
        |line| !line.trim().is_empty(),
    )
    .await?
    .into_bytes())
}

async fn translate_caption(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<u8>, FileError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FileError::Message("Only UTF-8 caption files are supported".to_string()))?;
    let should_translate = |line: &str| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && !trimmed.starts_with("WEBVTT")
            && !trimmed.contains("-->")
            && trimmed.parse::<usize>().is_err()
    };
    Ok(translate_lines(
        store,
        scheduler,
        text,
        options,
        output_mode,
        counts,
        progress,
        should_translate,
    )
    .await?
    .into_bytes())
}

#[allow(clippy::too_many_arguments)]
async fn translate_lines<F>(
    store: &AppStore,
    scheduler: &AiScheduler,
    text: &str,
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
    should_translate: F,
) -> Result<String, FileError>
where
    F: Fn(&str) -> bool,
{
    let mut lines = Vec::new();
    let mut candidates = Vec::new();
    for chunk in text.split_inclusive('\n') {
        let (line, suffix) = match chunk.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (chunk, ""),
        };
        let translate = should_translate(line);
        if translate {
            candidates.push(line.to_string());
        } else {
            counts.skipped += 1;
        }
        lines.push((line.to_string(), suffix.to_string(), translate));
    }
    let translations = translate_fragments(
        store,
        scheduler,
        candidates,
        options,
        output_mode,
        counts,
        progress,
    )
    .await?;
    let mut translated = String::with_capacity(text.len());
    let mut candidate_index = 0;
    for (line, suffix, should_translate) in lines {
        if should_translate {
            translated.push_str(&translations[candidate_index]);
            candidate_index += 1;
        } else {
            translated.push_str(&line);
        }
        translated.push_str(&suffix);
    }
    Ok(translated)
}

async fn translate_csv(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<u8>, FileError> {
    let delimiter = if bytes.contains(&b'\t') && !bytes.contains(&b',') {
        b'\t'
    } else {
        b','
    };
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(bytes);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut candidates = Vec::new();
    for row in reader.records() {
        let row = row?;
        let values = row.iter().map(str::to_string).collect::<Vec<_>>();
        for value in &values {
            if value.trim().is_empty() {
                counts.skipped += 1;
            } else {
                candidates.push(value.clone());
            }
        }
        rows.push(values);
    }
    let translations = translate_fragments(
        store,
        scheduler,
        candidates,
        options,
        output_mode,
        counts,
        progress,
    )
    .await?;
    let mut translation_index = 0;
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    for row in &mut rows {
        for value in &mut *row {
            if !value.trim().is_empty() {
                *value = translations[translation_index].clone();
                translation_index += 1;
            }
        }
        writer.write_record(row)?;
    }
    writer.flush()?;
    writer
        .into_inner()
        .map_err(|error| FileError::Io(error.into_error()))
}

async fn translate_json(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<u8>, FileError> {
    let mut json: Value = serde_json::from_slice(bytes)?;
    let mut candidates = Vec::new();
    collect_json_texts(&json, &mut candidates, &mut counts.skipped);
    let translations = translate_fragments(
        store,
        scheduler,
        candidates,
        options,
        output_mode,
        counts,
        progress,
    )
    .await?;
    let mut translation_index = 0;
    apply_json_texts(&mut json, &translations, &mut translation_index);
    Ok(serde_json::to_vec_pretty(&json)?)
}

fn collect_json_texts(value: &Value, texts: &mut Vec<String>, skipped: &mut usize) {
    match value {
        Value::String(text) if !text.trim().is_empty() => texts.push(text.clone()),
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => *skipped += 1,
        Value::Array(items) => {
            for item in items {
                collect_json_texts(item, texts, skipped);
            }
        }
        Value::Object(entries) => {
            for item in entries.values() {
                collect_json_texts(item, texts, skipped);
            }
        }
    }
}

fn apply_json_texts(value: &mut Value, translations: &[String], translation_index: &mut usize) {
    match value {
        Value::String(text) if !text.trim().is_empty() => {
            *text = translations[*translation_index].clone();
            *translation_index += 1;
        }
        Value::Array(items) => {
            for item in items {
                apply_json_texts(item, translations, translation_index);
            }
        }
        Value::Object(entries) => {
            for item in entries.values_mut() {
                apply_json_texts(item, translations, translation_index);
            }
        }
        _ => {}
    }
}

struct PlannedFragment {
    source: String,
    parts: Vec<FragmentPart>,
}

struct FragmentPart {
    result_index: usize,
    leading: String,
    trailing: String,
}

struct PendingPart {
    result_index: usize,
    text: String,
}

async fn translate_fragments(
    store: &AppStore,
    scheduler: &AiScheduler,
    fragments: Vec<String>,
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<String>, FileError> {
    let maximum = store.settings().max_chunk_chars.max(1);
    let mut planned = Vec::with_capacity(fragments.len());
    let mut pending = Vec::new();
    for source in fragments {
        let (_, content, _) = surrounding_whitespace(&source);
        if content.is_empty() {
            counts.skipped += 1;
            planned.push(PlannedFragment {
                source,
                parts: Vec::new(),
            });
            continue;
        }
        let mut parts = Vec::new();
        for chunk in split_chunks(content, maximum) {
            let (leading, text, trailing) = surrounding_whitespace(&chunk);
            if text.is_empty() {
                continue;
            }
            let result_index = pending.len();
            pending.push(PendingPart {
                result_index,
                text: text.to_string(),
            });
            parts.push(FragmentPart {
                result_index,
                leading: leading.to_string(),
                trailing: trailing.to_string(),
            });
        }
        planned.push(PlannedFragment { source, parts });
    }

    if pending.is_empty() {
        report(counts, progress);
        return Ok(planned
            .into_iter()
            .map(|fragment| fragment.source)
            .collect());
    }

    let mut batches: Vec<Vec<PendingPart>> = Vec::new();
    let mut current = Vec::new();
    let mut current_chars = 0usize;
    for part in pending {
        let part_chars = part.text.chars().count();
        if !current.is_empty() && current_chars + part_chars > maximum {
            batches.push(current);
            current = Vec::new();
            current_chars = 0;
        }
        current_chars += part_chars;
        current.push(part);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    counts.stage = "translating".to_string();
    counts.total_segments += planned
        .iter()
        .map(|fragment| fragment.parts.len())
        .sum::<usize>();
    counts.total_batches += batches.len();
    report(counts, progress);

    let mut results = vec![String::new(); counts.total_segments];
    let mut jobs = JoinSet::new();
    for batch in batches {
        let scheduler = scheduler.clone();
        let options = options.clone();
        jobs.spawn(async move {
            let indexes = batch
                .iter()
                .map(|part| part.result_index)
                .collect::<Vec<_>>();
            let texts = batch
                .iter()
                .map(|part| part.text.clone())
                .collect::<Vec<_>>();
            let translations = scheduler
                .translate_batch(options, texts, Priority::Low)
                .await
                .map_err(|error| FileError::Message(error.to_string()))?;
            if translations.len() != indexes.len() {
                return Err(FileError::Message(
                    "AI returned a different number of translations than requested".to_string(),
                ));
            }
            Ok::<_, FileError>((indexes, translations))
        });
    }
    while let Some(result) = jobs.join_next().await {
        let (indexes, translations) = result.map_err(|error| {
            FileError::Message(format!("Translation batch task failed: {error}"))
        })??;
        for (index, translation) in indexes.into_iter().zip(translations) {
            results[index] = translation;
            counts.translated += 1;
        }
        counts.completed_batches += 1;
        report(counts, progress);
    }

    let mut output = Vec::with_capacity(planned.len());
    for fragment in planned {
        if fragment.parts.is_empty() {
            output.push(fragment.source);
            continue;
        }
        let translated = fragment
            .parts
            .iter()
            .map(|part| {
                format!(
                    "{}{}{}",
                    part.leading, results[part.result_index], part.trailing
                )
            })
            .collect::<String>();
        output.push(render_fragment(&fragment.source, &translated, output_mode));
    }
    Ok(output)
}

fn render_fragment(source: &str, translated: &str, output_mode: FileOutputMode) -> String {
    if output_mode == FileOutputMode::Translated {
        return restore_surrounding_whitespace(source, translated);
    }
    let (leading, content, trailing) = surrounding_whitespace(source);
    if content.is_empty() {
        source.to_string()
    } else {
        format!(
            "{}{}\n{}{}{}",
            leading,
            content,
            leading,
            translated.trim(),
            trailing
        )
    }
}

fn restore_surrounding_whitespace(source: &str, translated: &str) -> String {
    let (leading, _, trailing) = surrounding_whitespace(source);
    format!("{}{}{}", leading, translated, trailing)
}

fn surrounding_whitespace(value: &str) -> (&str, &str, &str) {
    if value.trim().is_empty() {
        return (value, "", "");
    }
    let content_start = value.len() - value.trim_start().len();
    let content_end = value.trim_end().len();
    (
        &value[..content_start],
        &value[content_start..content_end],
        &value[content_end..],
    )
}

fn split_chunks(text: &str, maximum: usize) -> Vec<String> {
    if text.chars().count() <= maximum {
        return vec![text.to_string()];
    }
    let mut output = Vec::new();
    let mut remaining = text;
    while remaining.chars().count() > maximum {
        let boundary = remaining
            .char_indices()
            .nth(maximum)
            .map(|(index, _)| index)
            .unwrap_or(remaining.len());
        let candidate = &remaining[..boundary];
        let split = candidate
            .rfind(|character: char| {
                character == '\n'
                    || character == ' '
                    || character == '。'
                    || character == '.'
                    || character == '!'
                    || character == '?'
            })
            .filter(|index| *index > boundary / 3)
            .map(|index| index + remaining[index..].chars().next().unwrap().len_utf8())
            .unwrap_or(boundary);
        output.push(remaining[..split].to_string());
        remaining = &remaining[split..];
    }
    if !remaining.is_empty() {
        output.push(remaining.to_string());
    }
    output
}

#[derive(Clone, Copy)]
enum OfficeKind {
    Word,
    PowerPoint,
    Excel,
}

#[allow(clippy::too_many_arguments)]
async fn translate_office(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    kind: OfficeKind,
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<u8>, FileError> {
    let entries = unpack_office(bytes)?;
    let mut translated_entries = Vec::with_capacity(entries.len());
    for (name, is_directory, contents) in entries {
        if is_directory {
            translated_entries.push((name, true, Vec::new()));
            continue;
        }
        let translated = match kind {
            OfficeKind::Word if name.starts_with("word/") && name.ends_with(".xml") => {
                translate_xml_document(
                    store,
                    scheduler,
                    &contents,
                    options,
                    output_mode,
                    b"w:p",
                    b"w:t",
                    counts,
                    progress,
                )
                .await?
            }
            OfficeKind::PowerPoint if name.starts_with("ppt/slides/") && name.ends_with(".xml") => {
                translate_xml_document(
                    store,
                    scheduler,
                    &contents,
                    options,
                    output_mode,
                    b"a:p",
                    b"a:t",
                    counts,
                    progress,
                )
                .await?
            }
            OfficeKind::Excel if name == "xl/sharedStrings.xml" => {
                translate_xml_document(
                    store,
                    scheduler,
                    &contents,
                    options,
                    output_mode,
                    b"si",
                    b"t",
                    counts,
                    progress,
                )
                .await?
            }
            OfficeKind::Excel if name.starts_with("xl/worksheets/") && name.ends_with(".xml") => {
                translate_xml_document(
                    store,
                    scheduler,
                    &contents,
                    options,
                    output_mode,
                    b"is",
                    b"t",
                    counts,
                    progress,
                )
                .await?
            }
            _ if is_office_image(kind, &name) && provider_supports_images(store, options) => {
                translate_image_file(store, scheduler, &name, contents, options, counts, progress)
                    .await?
            }
            _ => {
                if is_office_image(kind, &name) {
                    counts.skipped += 1;
                }
                contents
            }
        };
        translated_entries.push((name, false, translated));
    }

    counts.stage = "packing".to_string();
    report(counts, progress);
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    let file_options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, is_directory, contents) in translated_entries {
        if is_directory {
            output.add_directory(name, file_options)?;
            continue;
        }
        output.start_file(name, file_options)?;
        output.write_all(&contents)?;
    }
    Ok(output.finish()?.into_inner())
}

fn is_office_image(kind: OfficeKind, name: &str) -> bool {
    let media_prefix = match kind {
        OfficeKind::Word => "word/media/",
        OfficeKind::PowerPoint => "ppt/media/",
        OfficeKind::Excel => "xl/media/",
    };
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    name.starts_with(media_prefix) && matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp")
}

fn provider_supports_images(store: &AppStore, options: &TranslateRequest) -> bool {
    store.snapshot().providers.iter().any(|provider| {
        provider.id == options.provider_id && provider.enabled && provider.supports_images
    })
}

fn unpack_office(bytes: &[u8]) -> Result<Vec<(String, bool, Vec<u8>)>, FileError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut source = archive.by_index(index)?;
        let name = source.name().to_string();
        let is_directory = source.is_dir();
        let mut contents = Vec::new();
        if !is_directory {
            source.read_to_end(&mut contents)?;
        }
        entries.push((name, is_directory, contents));
    }
    Ok(entries)
}

struct XmlBlock {
    start: usize,
    end: usize,
    original: Vec<u8>,
    text: String,
}

fn extract_xml_blocks(
    bytes: &[u8],
    block_tag: &[u8],
    text_tag: &[u8],
) -> Result<Vec<XmlBlock>, FileError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut block_start: Option<usize> = None;
    let mut depth = 0usize;
    let mut blocks = Vec::new();
    loop {
        let before = usize::try_from(reader.buffer_position())
            .map_err(|_| FileError::Message("Document XML exceeds supported size".to_string()))?;
        let event = reader.read_event_into(&mut buffer)?;
        match &event {
            Event::Start(start) if start.name().as_ref() == block_tag => {
                if block_start.is_none() {
                    block_start = Some(before);
                    depth = 1;
                } else {
                    depth += 1;
                }
            }
            Event::Start(_) if block_start.is_some() => depth += 1,
            Event::End(end) if block_start.is_some() => {
                depth = depth.saturating_sub(1);
                if depth == 0 && end.name().as_ref() == block_tag {
                    let end_position = usize::try_from(reader.buffer_position()).map_err(|_| {
                        FileError::Message("Document XML exceeds supported size".to_string())
                    })?;
                    let start = block_start.take().expect("block exists when depth is zero");
                    let original = bytes[start..end_position].to_vec();
                    let text = xml_text(&original, text_tag)?;
                    blocks.push(XmlBlock {
                        start,
                        end: end_position,
                        original,
                        text,
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(blocks)
}

#[allow(clippy::too_many_arguments)]
async fn translate_xml_document(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    block_tag: &[u8],
    text_tag: &[u8],
    counts: &mut Counts,
    progress: &ProgressCallback,
) -> Result<Vec<u8>, FileError> {
    let blocks = extract_xml_blocks(bytes, block_tag, text_tag)?;
    let sources = blocks
        .iter()
        .map(|block| block.text.clone())
        .collect::<Vec<_>>();
    let translations = translate_fragments(
        store,
        scheduler,
        sources,
        options,
        output_mode,
        counts,
        progress,
    )
    .await?;
    let mut output = Vec::with_capacity(bytes.len());
    let mut last_copy = 0;
    for (block, translated) in blocks.iter().zip(translations) {
        output.extend_from_slice(&bytes[last_copy..block.start]);
        if block.text.trim().is_empty() {
            output.extend_from_slice(&block.original);
        } else {
            output.extend_from_slice(&replace_xml_text(&block.original, text_tag, &translated)?);
        }
        last_copy = block.end;
    }
    output.extend_from_slice(&bytes[last_copy..]);
    Ok(output)
}

fn xml_text(xml: &[u8], text_tag: &[u8]) -> Result<String, FileError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut output = String::new();
    let mut inside = false;
    loop {
        match reader.read_event_into(&mut buffer)? {
            Event::Start(start) if start.name().as_ref() == text_tag => inside = true,
            Event::End(end) if end.name().as_ref() == text_tag => inside = false,
            Event::Text(text) if inside => {
                let raw = std::str::from_utf8(text.as_ref()).map_err(|_| {
                    FileError::Message("Document XML contains invalid UTF-8 text".to_string())
                })?;
                output.push_str(
                    &unescape(raw).map_err(|error| FileError::Message(error.to_string()))?,
                );
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(output)
}

fn replace_xml_text(xml: &[u8], text_tag: &[u8], translated: &str) -> Result<Vec<u8>, FileError> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut buffer = Vec::new();
    let mut inside = false;
    let mut wrote_translation = false;
    loop {
        let event = reader.read_event_into(&mut buffer)?;
        match event {
            Event::Start(start) if start.name().as_ref() == text_tag => {
                inside = true;
                writer.write_event(Event::Start(start.into_owned()))?;
            }
            Event::End(end) if end.name().as_ref() == text_tag => {
                inside = false;
                writer.write_event(Event::End(end.into_owned()))?;
            }
            Event::Text(_) if inside => {
                if !wrote_translation {
                    writer.write_event(Event::Text(BytesText::new(translated)))?;
                    wrote_translation = true;
                }
            }
            Event::Eof => break,
            other => writer.write_event(other.into_owned())?,
        }
        buffer.clear();
    }
    Ok(writer.into_inner())
}

fn translated_filename(filename: &str) -> String {
    match filename.rfind('.') {
        Some(index) if index > 0 => {
            format!("{}-translated{}", &filename[..index], &filename[index..])
        }
        _ => format!("{filename}-translated"),
    }
}

#[cfg(test)]
mod tests {
    use super::{render_fragment, replace_xml_text, split_chunks, surrounding_whitespace};
    use crate::models::FileOutputMode;

    #[test]
    fn splitting_preserves_all_content() {
        let source = "word ".repeat(100);
        let chunks = split_chunks(&source, 25);
        assert_eq!(chunks.concat(), source);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 25));
    }

    #[test]
    fn office_text_is_extracted_and_replaced_without_losing_runs() {
        let paragraph = br#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Hello </w:t></w:r><w:r><w:t>world &amp; team</w:t></w:r></w:p>"#;
        assert_eq!(
            super::xml_text(paragraph, b"w:t").unwrap(),
            "Hello world & team"
        );

        let translated = replace_xml_text(paragraph, b"w:t", "你好，团队").unwrap();
        let translated_text = super::xml_text(&translated, b"w:t").unwrap();
        assert_eq!(translated_text, "你好，团队");
        assert!(String::from_utf8(translated).unwrap().contains("<w:b"));
    }

    #[test]
    fn chunk_whitespace_can_be_restored_after_ai_trimming() {
        assert_eq!(
            surrounding_whitespace("  translate me \n"),
            ("  ", "translate me", " \n")
        );
        assert_eq!(surrounding_whitespace("   "), ("   ", "", ""));
    }

    #[test]
    fn bilingual_output_keeps_source_and_translation_whitespace() {
        assert_eq!(
            render_fragment("  hello ", "你好", FileOutputMode::Bilingual),
            "  hello\n  你好 "
        );
        assert_eq!(
            render_fragment("  hello ", "你好", FileOutputMode::Translated),
            "  你好 "
        );
    }
}
