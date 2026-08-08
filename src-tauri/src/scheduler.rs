use std::collections::VecDeque;

use tokio::sync::{mpsc, oneshot};

use crate::{
    ai::{self, AiError},
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
    ) -> Result<Vec<String>, AiError> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .send(Command::TranslateBatch {
                request,
                texts,
                response,
                priority,
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
    let mut active = 0usize;

    loop {
        while active < store.settings().max_concurrent_ai.max(1) {
            let Some(pending) = high.pop_front().or_else(|| low.pop_front()) else {
                break;
            };
            active += 1;
            let store = store.clone();
            let completion_sender = completion_sender.clone();
            tokio::spawn(async move {
                let completion = execute_work(store, pending.work).await;
                let _ = completion_sender.send(completion);
            });
        }

        if active == 0 && high.is_empty() && low.is_empty() {
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
                    active = active.saturating_sub(1);
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
        } => PendingWork {
            priority,
            work: Work::TranslateBatch {
                request,
                texts,
                response,
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

async fn execute_work(store: AppStore, work: Work) -> Completion {
    match work {
        Work::Translate { request, response } => {
            let result = ai::translate(&store, &request).await;
            Completion {
                work: Work::Translate { request, response },
                result: WorkResult::Translate(result),
            }
        }
        Work::TranslateBatch {
            request,
            texts,
            response,
        } => {
            let result = ai::translate_batch(&store, &request, &texts).await;
            Completion {
                work: Work::TranslateBatch {
                    request,
                    texts,
                    response,
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
