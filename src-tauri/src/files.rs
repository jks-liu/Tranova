use std::io::{Cursor, Read, Write};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use quick_xml::{
    escape::unescape,
    events::{BytesText, Event},
    Reader, Writer,
};
use serde_json::Value;
use thiserror::Error;
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

use crate::{
    ai,
    models::{FileJobResult, TranslateRequest},
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

#[derive(Default)]
struct Counts {
    translated: usize,
    skipped: usize,
}

pub async fn translate_file(
    store: &AppStore,
    filename: &str,
    bytes: Vec<u8>,
    options: TranslateRequest,
) -> Result<FileJobResult, FileError> {
    let extension = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut counts = Counts::default();
    let translated = match extension.as_str() {
        "docx" => translate_office(store, &bytes, &options, OfficeKind::Word, &mut counts).await?,
        "pptx" => translate_office(store, &bytes, &options, OfficeKind::PowerPoint, &mut counts).await?,
        "xlsx" => translate_office(store, &bytes, &options, OfficeKind::Excel, &mut counts).await?,
        "json" => translate_json(store, &bytes, &options, &mut counts).await?,
        "csv" => translate_csv(store, &bytes, &options, &mut counts).await?,
        "srt" | "vtt" => translate_caption(store, &bytes, &options, &mut counts).await?,
        "txt" | "md" | "markdown" | "html" | "htm" => translate_plain_file(store, &bytes, &options, &mut counts).await?,
        "png" | "jpg" | "jpeg" | "webp" => translate_image_file(store, filename, bytes, &options, &mut counts).await?,
        _ => return Err(FileError::Message("Unsupported file type. Use DOCX, PPTX, XLSX, TXT, Markdown, HTML, CSV, JSON, SRT, VTT, PNG, JPG or WebP.".to_string())),
    };
    Ok(FileJobResult {
        filename: translated_filename(filename),
        media_type: mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string(),
        content_base64: BASE64.encode(translated),
        translated_segments: counts.translated,
        skipped_segments: counts.skipped,
    })
}

async fn translate_image_file(
    store: &AppStore,
    filename: &str,
    bytes: Vec<u8>,
    options: &TranslateRequest,
    counts: &mut Counts,
) -> Result<Vec<u8>, FileError> {
    let translated = ai::translate_image(store, options, filename, bytes)
        .await
        .map_err(|error| FileError::Message(error.to_string()))?;
    counts.translated += 1;
    Ok(translated)
}

async fn translate_plain_file(
    store: &AppStore,
    bytes: &[u8],
    options: &TranslateRequest,
    counts: &mut Counts,
) -> Result<Vec<u8>, FileError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FileError::Message("Only UTF-8 text files are supported".to_string()))?;
    Ok(
        translate_lines(store, text, options, counts, |line| !line.trim().is_empty())
            .await?
            .into_bytes(),
    )
}

async fn translate_caption(
    store: &AppStore,
    bytes: &[u8],
    options: &TranslateRequest,
    counts: &mut Counts,
) -> Result<Vec<u8>, FileError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FileError::Message("Only UTF-8 caption files are supported".to_string()))?;
    let should_translate = |line: &str| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && !trimmed.starts_with("WEBVTT")
            && !trimmed.contains("-->")
            && !trimmed.parse::<usize>().is_ok()
    };
    Ok(
        translate_lines(store, text, options, counts, should_translate)
            .await?
            .into_bytes(),
    )
}

async fn translate_lines<F>(
    store: &AppStore,
    text: &str,
    options: &TranslateRequest,
    counts: &mut Counts,
    should_translate: F,
) -> Result<String, FileError>
where
    F: Fn(&str) -> bool,
{
    let mut translated = String::with_capacity(text.len());
    for chunk in text.split_inclusive('\n') {
        let (line, suffix) = match chunk.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (chunk, ""),
        };
        if should_translate(line) {
            translated.push_str(
                &translate_preserving_surrounding_whitespace(store, line, options, counts).await?,
            );
        } else {
            counts.skipped += 1;
            translated.push_str(line);
        }
        translated.push_str(suffix);
    }
    Ok(translated)
}

async fn translate_csv(
    store: &AppStore,
    bytes: &[u8],
    options: &TranslateRequest,
    counts: &mut Counts,
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
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    for row in reader.records() {
        let row = row?;
        let mut output = csv::StringRecord::new();
        for value in &row {
            if value.trim().is_empty() {
                counts.skipped += 1;
                output.push_field(value);
            } else {
                output.push_field(&translate_fragment(store, value, options, counts).await?);
            }
        }
        writer.write_record(&output)?;
    }
    writer.flush()?;
    writer
        .into_inner()
        .map_err(|error| FileError::Io(error.into_error()))
}

async fn translate_json(
    store: &AppStore,
    bytes: &[u8],
    options: &TranslateRequest,
    counts: &mut Counts,
) -> Result<Vec<u8>, FileError> {
    let mut json: Value = serde_json::from_slice(bytes)?;
    translate_json_value(store, &mut json, options, counts).await?;
    Ok(serde_json::to_vec_pretty(&json)?)
}

async fn translate_json_value(
    store: &AppStore,
    value: &mut Value,
    options: &TranslateRequest,
    counts: &mut Counts,
) -> Result<(), FileError> {
    match value {
        Value::String(text) => {
            if text.trim().is_empty() {
                counts.skipped += 1;
            } else {
                *text = translate_fragment(store, text, options, counts).await?;
            }
        }
        Value::Array(items) => {
            for item in items {
                Box::pin(translate_json_value(store, item, options, counts)).await?;
            }
        }
        Value::Object(entries) => {
            for value in entries.values_mut() {
                Box::pin(translate_json_value(store, value, options, counts)).await?;
            }
        }
        _ => counts.skipped += 1,
    }
    Ok(())
}

async fn translate_preserving_surrounding_whitespace(
    store: &AppStore,
    line: &str,
    options: &TranslateRequest,
    counts: &mut Counts,
) -> Result<String, FileError> {
    let leading = line.len() - line.trim_start().len();
    let trailing = line.len() - line.trim_end().len();
    let text = line.trim();
    let translated = translate_fragment(store, text, options, counts).await?;
    Ok(format!(
        "{}{}{}",
        &line[..leading],
        translated,
        &line[line.len() - trailing..]
    ))
}

async fn translate_fragment(
    store: &AppStore,
    text: &str,
    options: &TranslateRequest,
    counts: &mut Counts,
) -> Result<String, FileError> {
    let max_chars = store.settings().max_chunk_chars;
    let chunks = split_chunks(text, max_chars);
    let mut result = String::new();
    for chunk in chunks {
        let (leading, content, trailing) = surrounding_whitespace(&chunk);
        if content.is_empty() {
            result.push_str(&chunk);
            continue;
        }
        let mut request = options.clone();
        request.text = content.to_string();
        let response = ai::translate(store, &request)
            .await
            .map_err(|error| FileError::Message(error.to_string()))?;
        counts.translated += 1;
        result.push_str(leading);
        result.push_str(&response.translated_text);
        result.push_str(trailing);
    }
    Ok(result)
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

async fn translate_office(
    store: &AppStore,
    bytes: &[u8],
    options: &TranslateRequest,
    kind: OfficeKind,
    counts: &mut Counts,
) -> Result<Vec<u8>, FileError> {
    // ZipArchive and ZipFile are not Send; fully unpack before awaiting AI responses.
    let entries = unpack_office(bytes)?;
    let mut translated_entries = Vec::with_capacity(entries.len());
    for (name, is_directory, contents) in entries {
        if is_directory {
            translated_entries.push((name, true, Vec::new()));
            continue;
        }
        let translated = match kind {
            OfficeKind::Word if name.starts_with("word/") && name.ends_with(".xml") => {
                translate_xml_document(store, &contents, options, b"w:p", b"w:t", counts).await?
            }
            OfficeKind::PowerPoint if name.starts_with("ppt/slides/") && name.ends_with(".xml") => {
                translate_xml_document(store, &contents, options, b"a:p", b"a:t", counts).await?
            }
            OfficeKind::Excel if name == "xl/sharedStrings.xml" => {
                translate_xml_document(store, &contents, options, b"si", b"t", counts).await?
            }
            OfficeKind::Excel if name.starts_with("xl/worksheets/") && name.ends_with(".xml") => {
                translate_xml_document(store, &contents, options, b"is", b"t", counts).await?
            }
            _ if is_office_image(kind, &name) && provider_supports_images(store, options) => {
                translate_image_file(store, &name, contents, options, counts).await?
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

async fn translate_xml_document(
    store: &AppStore,
    bytes: &[u8],
    options: &TranslateRequest,
    block_tag: &[u8],
    text_tag: &[u8],
    counts: &mut Counts,
) -> Result<Vec<u8>, FileError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut last_copy = 0;
    let mut block_start: Option<usize> = None;
    let mut depth = 0usize;
    let mut output = Vec::with_capacity(bytes.len());
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
                    output.extend_from_slice(&bytes[last_copy..start]);
                    let original = &bytes[start..end_position];
                    let text = xml_text(original, text_tag)?;
                    if text.trim().is_empty() {
                        counts.skipped += 1;
                        output.extend_from_slice(original);
                    } else {
                        let translated = translate_fragment(store, &text, options, counts).await?;
                        output.extend_from_slice(&replace_xml_text(
                            original,
                            text_tag,
                            &translated,
                        )?);
                    }
                    last_copy = end_position;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
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
    use super::{replace_xml_text, split_chunks, surrounding_whitespace, xml_text};

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
        assert_eq!(xml_text(paragraph, b"w:t").unwrap(), "Hello world & team");

        let translated = replace_xml_text(paragraph, b"w:t", "你好，团队").unwrap();
        let translated_text = xml_text(&translated, b"w:t").unwrap();
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
}
