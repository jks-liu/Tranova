use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::RwLock;

use crate::{
    files::FileTranslationResult,
    models::{FileJobState, FileJobStatus, FileOutputMode, FileProgress},
};

#[derive(Clone, Default)]
pub struct FileJobManager {
    inner: Arc<RwLock<HashMap<String, JobRecord>>>,
}

struct JobRecord {
    status: FileJobStatus,
    content: Option<Vec<u8>>,
}

impl FileJobManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self, filename: &str, output_mode: FileOutputMode) -> FileJobStatus {
        let id = job_id();
        let status = FileJobStatus {
            id: id.clone(),
            filename: filename.to_string(),
            output_mode,
            state: FileJobState::Queued,
            stage: "queued".to_string(),
            total_segments: 0,
            translated_segments: 0,
            skipped_segments: 0,
            total_batches: 0,
            completed_batches: 0,
            result_filename: translated_filename(filename),
            media_type: mime_guess::from_path(filename)
                .first_or_octet_stream()
                .to_string(),
            error: None,
            created_at: unix_timestamp(),
        };
        self.inner.write().insert(
            id,
            JobRecord {
                status: status.clone(),
                content: None,
            },
        );
        status
    }

    pub fn list(&self) -> Vec<FileJobStatus> {
        let mut jobs = self
            .inner
            .read()
            .values()
            .map(|record| record.status.clone())
            .collect::<Vec<_>>();
        jobs.sort_by(|left, right| right.id.cmp(&left.id));
        jobs
    }

    pub fn get(&self, id: &str) -> Option<FileJobStatus> {
        self.inner
            .read()
            .get(id)
            .map(|record| record.status.clone())
    }

    pub fn mark_processing(&self, id: &str) {
        if let Some(record) = self.inner.write().get_mut(id) {
            record.status.state = FileJobState::Processing;
            record.status.stage = "preparing".to_string();
        }
    }

    pub fn update_progress(&self, id: &str, progress: FileProgress) {
        if let Some(record) = self.inner.write().get_mut(id) {
            record.status.state = FileJobState::Processing;
            record.status.stage = progress.stage;
            record.status.total_segments = progress.total_segments;
            record.status.translated_segments = progress.translated_segments;
            record.status.skipped_segments = progress.skipped_segments;
            record.status.total_batches = progress.total_batches;
            record.status.completed_batches = progress.completed_batches;
        }
    }

    pub fn complete(&self, id: &str, result: FileTranslationResult) {
        if let Some(record) = self.inner.write().get_mut(id) {
            record.status.state = FileJobState::Completed;
            record.status.stage = "completed".to_string();
            record.status.total_segments = result.total_segments;
            record.status.translated_segments = result.translated_segments;
            record.status.skipped_segments = result.skipped_segments;
            record.status.total_batches = result.total_batches;
            record.status.completed_batches = result.total_batches;
            record.status.result_filename = result.filename;
            record.status.media_type = result.media_type;
            record.status.error = None;
            record.content = Some(result.content);
        }
    }

    pub fn fail(&self, id: &str, error: String) {
        if let Some(record) = self.inner.write().get_mut(id) {
            record.status.state = FileJobState::Failed;
            record.status.stage = "failed".to_string();
            record.status.error = Some(error);
        }
    }

    pub fn download(&self, id: &str) -> Option<(String, String, Vec<u8>)> {
        let jobs = self.inner.read();
        let record = jobs.get(id)?;
        if !matches!(record.status.state, FileJobState::Completed) {
            return None;
        }
        Some((
            record.status.result_filename.clone(),
            record.status.media_type.clone(),
            record.content.clone()?,
        ))
    }
}

fn job_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("file-{nanos}-{}", std::process::id())
}

fn unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn translated_filename(filename: &str) -> String {
    match filename.rfind('.') {
        Some(index) if index > 0 => {
            format!("{}-translated{}", &filename[..index], &filename[index..])
        }
        _ => format!("{filename}-translated"),
    }
}
