use std::{
    collections::HashMap,
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
use tokio::{
    sync::mpsc,
    task::JoinSet,
    time::{sleep, Duration},
};
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

use crate::{
    ai::StreamCallback,
    models::{FileBatchFailure, FileOutputMode, FileProgress, TranslateRequest},
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
    pub failed_segments: usize,
    pub failed_batches: Vec<FileBatchFailure>,
    pub retry_context: Option<FileRetryContext>,
}

#[derive(Debug, Clone)]
pub struct FileRetryContext {
    filename: String,
    bytes: Vec<u8>,
    options: TranslateRequest,
    output_mode: FileOutputMode,
    cached_batches: HashMap<usize, CachedBatch>,
}

#[derive(Debug, Clone)]
enum CachedBatch {
    Text(Vec<String>),
    Image(Vec<u8>),
}

#[derive(Default)]
struct BatchState {
    cached_batches: HashMap<usize, CachedBatch>,
    failed_batches: Vec<FileBatchFailure>,
}

#[derive(Default)]
struct Counts {
    translated: usize,
    failed: usize,
    skipped: usize,
    total_segments: usize,
    total_batches: usize,
    completed_batches: usize,
    stage: String,
    streaming_batch: Option<usize>,
    streaming_segments: usize,
    streaming_batch_segments: usize,
    streaming_text: Option<String>,
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
    translate_file_with_cache(
        store,
        scheduler,
        filename,
        bytes,
        options,
        output_mode,
        progress,
        HashMap::new(),
    )
    .await
}

pub async fn retry_failed_batches(
    store: &AppStore,
    scheduler: &AiScheduler,
    context: FileRetryContext,
    progress: ProgressCallback,
) -> Result<FileTranslationResult, FileError> {
    let FileRetryContext {
        filename,
        bytes,
        options,
        output_mode,
        cached_batches,
    } = context;
    translate_file_with_cache(
        store,
        scheduler,
        &filename,
        bytes,
        options,
        output_mode,
        progress,
        cached_batches,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn translate_file_with_cache(
    store: &AppStore,
    scheduler: &AiScheduler,
    filename: &str,
    bytes: Vec<u8>,
    options: TranslateRequest,
    output_mode: FileOutputMode,
    progress: ProgressCallback,
    cached_batches: HashMap<usize, CachedBatch>,
) -> Result<FileTranslationResult, FileError> {
    let source_bytes = bytes.clone();
    let mut batch_state = BatchState {
        cached_batches,
        ..BatchState::default()
    };
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
                &mut batch_state,
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
                &mut batch_state,
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
                &mut batch_state,
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
                &mut batch_state,
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
                &mut batch_state,
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
                &mut batch_state,
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
                &mut batch_state,
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
                &mut batch_state,
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
    counts.streaming_batch = None;
    counts.streaming_segments = 0;
    counts.streaming_batch_segments = 0;
    counts.streaming_text = None;
    report(&counts, &progress);
    let retry_context = if batch_state.failed_batches.is_empty() {
        None
    } else {
        Some(FileRetryContext {
            filename: filename.to_string(),
            bytes: source_bytes,
            options: options.clone(),
            output_mode,
            cached_batches: batch_state.cached_batches,
        })
    };
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
        failed_segments: counts.failed,
        failed_batches: batch_state.failed_batches,
        retry_context,
    })
}

fn report(counts: &Counts, progress: &ProgressCallback) {
    progress(FileProgress {
        stage: counts.stage.clone(),
        total_segments: counts.total_segments,
        translated_segments: counts.translated,
        failed_segments: counts.failed,
        skipped_segments: counts.skipped,
        total_batches: counts.total_batches,
        completed_batches: counts.completed_batches,
        streaming_batch: counts.streaming_batch,
        streaming_segments: counts.streaming_segments,
        streaming_batch_segments: counts.streaming_batch_segments,
        streaming_text: counts.streaming_text.clone(),
    });
}

#[allow(clippy::too_many_arguments)]
async fn translate_image_file(
    store: &AppStore,
    scheduler: &AiScheduler,
    filename: &str,
    bytes: Vec<u8>,
    options: &TranslateRequest,
    counts: &mut Counts,
    progress: &ProgressCallback,
    state: &mut BatchState,
) -> Result<Vec<u8>, FileError> {
    counts.stage = "translating".to_string();
    let batch_id = counts.total_batches;
    counts.total_segments += 1;
    counts.total_batches += 1;
    report(counts, progress);
    if let Some(CachedBatch::Image(translated)) = state.cached_batches.get(&batch_id) {
        counts.translated += 1;
        counts.completed_batches += 1;
        report(counts, progress);
        return Ok(translated.clone());
    }
    let max_retries = store.settings().max_batch_retries;
    let mut attempts = 0;
    let translated = loop {
        attempts += 1;
        match scheduler
            .translate_image(
                options.clone(),
                filename.to_string(),
                bytes.clone(),
                Priority::Low,
            )
            .await
        {
            Ok(translated) => break Ok(translated),
            Err(_error) if attempts <= max_retries => {
                sleep(Duration::from_millis((250 * attempts as u64).min(2_000))).await;
            }
            Err(error) => break Err(error.to_string()),
        }
    };
    let translated = match translated {
        Ok(translated) => {
            state
                .cached_batches
                .insert(batch_id, CachedBatch::Image(translated.clone()));
            counts.translated += 1;
            translated
        }
        Err(error) => {
            state.failed_batches.push(FileBatchFailure {
                id: batch_id,
                segment_count: 1,
                attempts,
                error,
            });
            counts.failed += 1;
            bytes
        }
    };
    counts.completed_batches += 1;
    report(counts, progress);
    Ok(translated)
}

#[allow(clippy::too_many_arguments)]
async fn translate_plain_file(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
    state: &mut BatchState,
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
        state,
        |line| !line.trim().is_empty(),
    )
    .await?
    .into_bytes())
}

#[allow(clippy::too_many_arguments)]
async fn translate_caption(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
    state: &mut BatchState,
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
        state,
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
    state: &mut BatchState,
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
        state,
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

#[allow(clippy::too_many_arguments)]
async fn translate_csv(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
    state: &mut BatchState,
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
        state,
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

#[allow(clippy::too_many_arguments)]
async fn translate_json(
    store: &AppStore,
    scheduler: &AiScheduler,
    bytes: &[u8],
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
    state: &mut BatchState,
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
        state,
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

#[allow(clippy::too_many_arguments)]
async fn translate_fragments(
    store: &AppStore,
    scheduler: &AiScheduler,
    fragments: Vec<String>,
    options: &TranslateRequest,
    output_mode: FileOutputMode,
    counts: &mut Counts,
    progress: &ProgressCallback,
    state: &mut BatchState,
) -> Result<Vec<String>, FileError> {
    let settings = store.settings();
    let maximum = settings.max_chunk_chars.max(1);
    let maximum_segments = settings.max_chunk_segments.max(1);
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

    let pending_count = pending.len();
    let batch_base = counts.total_batches;
    let mut batches: Vec<(usize, Vec<PendingPart>)> = Vec::new();
    let mut current = Vec::new();
    let mut current_chars = 0usize;
    for part in pending {
        let part_chars = part.text.chars().count();
        if !current.is_empty()
            && (current_chars + part_chars > maximum || current.len() >= maximum_segments)
        {
            let batch_id = batch_base + batches.len();
            batches.push((batch_id, current));
            current = Vec::new();
            current_chars = 0;
        }
        current_chars += part_chars;
        current.push(part);
    }
    if !current.is_empty() {
        let batch_id = batch_base + batches.len();
        batches.push((batch_id, current));
    }
    counts.stage = "translating".to_string();
    counts.total_segments += planned
        .iter()
        .map(|fragment| fragment.parts.len())
        .sum::<usize>();
    counts.total_batches += batches.len();
    report(counts, progress);

    let mut results = vec![String::new(); pending_count];
    let (stream_sender, mut stream_receiver) = mpsc::unbounded_channel();
    let mut jobs = JoinSet::new();
    let mut pending_jobs = 0usize;
    for (batch_id, batch) in batches {
        let indexes = batch
            .iter()
            .map(|part| part.result_index)
            .collect::<Vec<_>>();
        let texts = batch
            .iter()
            .map(|part| part.text.clone())
            .collect::<Vec<_>>();
        let cached = state.cached_batches.get(&batch_id).cloned();
        if let Some(CachedBatch::Text(translations)) = cached {
            if translations.len() == indexes.len() {
                for (index, translation) in indexes.into_iter().zip(translations) {
                    results[index] = translation;
                    counts.translated += 1;
                }
                counts.completed_batches += 1;
                report(counts, progress);
                continue;
            }
            state.cached_batches.remove(&batch_id);
        } else if cached.is_some() {
            state.cached_batches.remove(&batch_id);
        }

        let scheduler = scheduler.clone();
        let options = options.clone();
        let stream_sender = stream_sender.clone();
        let max_retries = settings.max_batch_retries;
        let batch_segments = indexes.len();
        pending_jobs += 1;
        jobs.spawn(async move {
            let stream_callback: StreamCallback = Arc::new(move |text| {
                let _ = stream_sender.send((batch_id, batch_segments, text));
            });
            let mut attempts = 0usize;
            let result = loop {
                attempts += 1;
                match scheduler
                    .translate_batch(
                        options.clone(),
                        texts.clone(),
                        Priority::Low,
                        Some(stream_callback.clone()),
                    )
                    .await
                {
                    Ok(translations) if translations.len() == texts.len() => {
                        break Ok(translations)
                    }
                    Ok(translations) => {
                        let error = format!(
                            "AI returned {} translations for {} input segments",
                            translations.len(),
                            texts.len()
                        );
                        if attempts > max_retries {
                            break Err(error);
                        }
                    }
                    Err(error) => {
                        if attempts > max_retries {
                            break Err(error.to_string());
                        }
                    }
                }
                sleep(Duration::from_millis((250 * attempts as u64).min(2_000))).await;
            };
            Ok::<_, FileError>((batch_id, indexes, texts, attempts, result))
        });
    }

    drop(stream_sender);
    while pending_jobs > 0 {
        if stream_receiver.is_closed() {
            let result = jobs.join_next().await.ok_or_else(|| {
                FileError::Message("Translation batch task ended unexpectedly".to_string())
            })?;
            let (batch_id, indexes, texts, attempts, result) = result.map_err(|error| {
                FileError::Message(format!("Translation batch task failed: {error}"))
            })??;
            apply_batch_result(
                state,
                counts,
                progress,
                batch_id,
                indexes,
                texts,
                attempts,
                result,
                &mut results,
            );
            pending_jobs -= 1;
            continue;
        }
        tokio::select! {
            stream_update = stream_receiver.recv() => {
                if let Some((batch_id, batch_segments, text)) = stream_update {
                    counts.streaming_batch = Some(batch_id);
                    counts.streaming_segments = partial_batch_item_count(&text);
                    counts.streaming_batch_segments = batch_segments;
                    counts.streaming_text = Some(stream_preview(&text));
                    report(counts, progress);
                }
            }
            result = jobs.join_next() => {
                let result = result.ok_or_else(|| {
                    FileError::Message("Translation batch task ended unexpectedly".to_string())
                })?;
                let (batch_id, indexes, texts, attempts, result) = result.map_err(|error| {
                    FileError::Message(format!("Translation batch task failed: {error}"))
                })??;
                apply_batch_result(
                    state,
                    counts,
                    progress,
                    batch_id,
                    indexes,
                    texts,
                    attempts,
                    result,
                    &mut results,
                );
                pending_jobs -= 1;
            }
        }
    }
    counts.streaming_batch = None;
    counts.streaming_segments = 0;
    counts.streaming_batch_segments = 0;
    counts.streaming_text = None;
    report(counts, progress);

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

#[allow(clippy::too_many_arguments)]
fn apply_batch_result(
    state: &mut BatchState,
    counts: &mut Counts,
    progress: &ProgressCallback,
    batch_id: usize,
    indexes: Vec<usize>,
    texts: Vec<String>,
    attempts: usize,
    result: Result<Vec<String>, String>,
    results: &mut [String],
) {
    counts.streaming_batch = None;
    counts.streaming_segments = 0;
    counts.streaming_batch_segments = 0;
    counts.streaming_text = None;
    match result {
        Ok(translations) => {
            for (index, translation) in indexes.iter().copied().zip(&translations) {
                results[index] = translation.clone();
                counts.translated += 1;
            }
            state
                .cached_batches
                .insert(batch_id, CachedBatch::Text(translations));
        }
        Err(error) => {
            for (index, text) in indexes.into_iter().zip(&texts) {
                results[index] = text.clone();
            }
            counts.failed += texts.len();
            state.failed_batches.push(FileBatchFailure {
                id: batch_id,
                segment_count: texts.len(),
                attempts,
                error,
            });
        }
    }
    counts.completed_batches += 1;
    report(counts, progress);
}

fn stream_preview(text: &str) -> String {
    const MAX_STREAM_PREVIEW_CHARS: usize = 4_000;
    let total = text.chars().count();
    if total <= MAX_STREAM_PREVIEW_CHARS {
        text.to_string()
    } else {
        text.chars()
            .skip(total - MAX_STREAM_PREVIEW_CHARS)
            .collect()
    }
}

fn partial_batch_item_count(text: &str) -> usize {
    let Some(start) = text.find('[') else {
        return 0;
    };
    let mut escaped = false;
    let mut in_string = false;
    let mut count = 0;
    for character in text[start + 1..].chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
                count += 1;
            }
        } else if character == '"' {
            in_string = true;
        }
    }
    count
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
    state: &mut BatchState,
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
                    state,
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
                    state,
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
                    state,
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
                    state,
                )
                .await?
            }
            _ if is_office_image(kind, &name) && provider_supports_images(store, options) => {
                translate_image_file(
                    store, scheduler, &name, contents, options, counts, progress, state,
                )
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
    state: &mut BatchState,
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
        state,
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
