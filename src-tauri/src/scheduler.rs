use std::collections::VecDeque;

use tokio::sync::{mpsc, oneshot};

use crate::{
    ai::{self, AiError, StreamCallback},
    models::{TranslateRequest, TranslationResult},
    store::AppStore,
};

#[derive(Clone, Copy)]
pub enum Priority {
    High,
    Low,
}

#[derive(Clone)]
pub struct AiScheduler {
    sender: mpsc::UnboundedSender<Command>,
}

enum Command {
    Translate {
        request: TranslateRequest,
        response: oneshot::Sender<Result<TranslationResult, AiError>>,
        priority: Priority,
    },
    TranslateBatch {
        request: TranslateRequest,
        texts: Vec<String>,
        response: oneshot::Sender<Result<Vec<String>, AiError>>,
        priority: Priority,
        stream: Option<StreamCallback>,
    },
    TranslateImage {
        request: TranslateRequest,
        filename: String,
        image: Vec<u8>,
        response: oneshot::Sender<Result<Vec<u8>, AiError>>,
        priority: Priority,
    },
}

enum Work {
    Translate {
        request: TranslateRequest,
        response: oneshot::Sender<Result<TranslationResult, AiError>>,
    },
    TranslateBatch {
        request: TranslateRequest,
        texts: Vec<String>,
        response: oneshot::Sender<Result<Vec<String>, AiError>>,
        stream: Option<StreamCallback>,
    },
    TranslateImage {
        request: TranslateRequest,
        filename: String,
        image: Vec<u8>,
        response: oneshot::Sender<Result<Vec<u8>, AiError>>,
    },
}

impl AiScheduler {
    pub fn new(store: AppStore) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        tokio::spawn(run_scheduler(store, receiver));
        Self { sender }
    }

    pub async fn translate(
        &self,
        request: TranslateRequest,
        priority: Priority,
    ) -> Result<TranslationResult, AiError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(Command::Translate {
                request,
                response,
                priority,
            })
            .map_err(|_| AiError::Message("AI scheduler is unavailable".to_string()))?;
        receiver
            .await
            .map_err(|_| AiError::Message("AI scheduler stopped unexpectedly".to_string()))?
    }

    pub async fn translate_batch(
        &self,
        request: TranslateRequest,
        texts: Vec<String>,
        priority: Priority,
        stream: Option<StreamCallback>,
    ) -> Result<Vec<String>, AiError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(Command::TranslateBatch {
                request,
                texts,
                response,
                priority,
                stream,
            })
            .map_err(|_| AiError::Message("AI scheduler is unavailable".to_string()))?;
        receiver
            .await
            .map_err(|_| AiError::Message("AI scheduler stopped unexpectedly".to_string()))?
    }

    pub async fn translate_image(
        &self,
        request: TranslateRequest,
        filename: String,
        image: Vec<u8>,
        priority: Priority,
    ) -> Result<Vec<u8>, AiError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(Command::TranslateImage {
                request,
                filename,
                image,
                response,
                priority,
            })
            .map_err(|_| AiError::Message("AI scheduler is unavailable".to_string()))?;
        receiver
            .await
            .map_err(|_| AiError::Message("AI scheduler stopped unexpectedly".to_string()))?
    }
}

struct Completion {
    priority: Priority,
    work: Work,
    result: WorkResult,
}

enum WorkResult {
    Translate(Result<TranslationResult, AiError>),
    TranslateBatch(Result<Vec<String>, AiError>),
    TranslateImage(Result<Vec<u8>, AiError>),
}

struct PendingWork {
    priority: Priority,
    work: Work,
}

async fn run_scheduler(store: AppStore, mut receiver: mpsc::UnboundedReceiver<Command>) {
    let (completion_sender, mut completion_receiver) = mpsc::unbounded_channel();
    let mut high: VecDeque<PendingWork> = VecDeque::new();
    let mut low: VecDeque<PendingWork> = VecDeque::new();
    let mut active_high = 0usize;
    let mut active_low = 0usize;

    loop {
        let max_active = store.settings().max_concurrent_ai.max(1);
        let max_low_active = max_active.saturating_sub(1).max(1);
        while active_high + active_low < max_active {
            let pending = if let Some(pending) = high.pop_front() {
                pending
            } else if active_low < max_low_active {
                let Some(pending) = low.pop_front() else {
                    break;
                };
                pending
            } else {
                break;
            };
            match pending.priority {
                Priority::High => active_high += 1,
                Priority::Low => active_low += 1,
            }
            let priority = pending.priority;
            let store = store.clone();
            let completion_sender = completion_sender.clone();
            tokio::spawn(async move {
                let completion = execute_work(store, priority, pending.work).await;
                let _ = completion_sender.send(completion);
            });
        }

        if active_high + active_low == 0 && high.is_empty() && low.is_empty() {
            let Some(command) = receiver.recv().await else {
                return;
            };
            enqueue(command, &mut high, &mut low);
            continue;
        }

        tokio::select! {
            command = receiver.recv() => {
                if let Some(command) = command {
                    enqueue(command, &mut high, &mut low);
                } else {
                    return;
                }
            }
            completion = completion_receiver.recv() => {
                if let Some(completion) = completion {
                    match completion.priority {
                        Priority::High => active_high = active_high.saturating_sub(1),
                        Priority::Low => active_low = active_low.saturating_sub(1),
                    }
                    send_completion(completion);
                }
            }
        }
    }
}

fn enqueue(command: Command, high: &mut VecDeque<PendingWork>, low: &mut VecDeque<PendingWork>) {
    let pending = match command {
        Command::Translate {
            request,
            response,
            priority,
        } => PendingWork {
            priority,
            work: Work::Translate { request, response },
        },
        Command::TranslateBatch {
            request,
            texts,
            response,
            priority,
            stream,
        } => PendingWork {
            priority,
            work: Work::TranslateBatch {
                request,
                texts,
                response,
                stream,
            },
        },
        Command::TranslateImage {
            request,
            filename,
            image,
            response,
            priority,
        } => PendingWork {
            priority,
            work: Work::TranslateImage {
                request,
                filename,
                image,
                response,
            },
        },
    };
    match pending.priority {
        Priority::High => high.push_back(pending),
        Priority::Low => low.push_back(pending),
    }
}

async fn execute_work(store: AppStore, priority: Priority, work: Work) -> Completion {
    match work {
        Work::Translate { request, response } => {
            let result = ai::translate(&store, &request).await;
            Completion {
                priority,
                work: Work::Translate { request, response },
                result: WorkResult::Translate(result),
            }
        }
        Work::TranslateBatch {
            request,
            texts,
            response,
            stream,
        } => {
            let result = ai::translate_batch(&store, &request, &texts, stream.clone()).await;
            Completion {
                priority,
                work: Work::TranslateBatch {
                    request,
                    texts,
                    response,
                    stream,
                },
                result: WorkResult::TranslateBatch(result),
            }
        }
        Work::TranslateImage {
            request,
            filename,
            image,
            response,
        } => {
            let result = ai::translate_image(&store, &request, &filename, image).await;
            Completion {
                priority,
                work: Work::TranslateImage {
                    request,
                    filename,
                    image: Vec::new(),
                    response,
                },
                result: WorkResult::TranslateImage(result),
            }
        }
    }
}

fn send_completion(completion: Completion) {
    match (completion.work, completion.result) {
        (Work::Translate { response, .. }, WorkResult::Translate(result)) => {
            let _ = response.send(result);
        }
        (Work::TranslateBatch { response, .. }, WorkResult::TranslateBatch(result)) => {
            let _ = response.send(result);
        }
        (Work::TranslateImage { response, .. }, WorkResult::TranslateImage(result)) => {
            let _ = response.send(result);
        }
        _ => {}
    }
}
