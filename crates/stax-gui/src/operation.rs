#![allow(dead_code)]

use gpui::App;
use stax::application::{
    OperationEvent, OperationReporter, OperationRequest, OperationResult,
    execute_repository_operation,
};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use url::Url;

pub type OperationFuture = Pin<Box<dyn Future<Output = OperationResult> + Send + 'static>>;

pub trait OperationService: Send + Sync {
    fn execute(
        &self,
        repository_root: PathBuf,
        request: OperationRequest,
        events: async_channel::Sender<OperationEvent>,
    ) -> OperationFuture;
}

pub trait BrowserService: Send + Sync {
    fn open_url(&self, url: &str, cx: &mut App) -> Result<(), String>;
}

pub struct NativeOperationService;

impl OperationService for NativeOperationService {
    fn execute(
        &self,
        repository_root: PathBuf,
        request: OperationRequest,
        events: async_channel::Sender<OperationEvent>,
    ) -> OperationFuture {
        Box::pin(async move {
            let mut reporter = ChannelOperationReporter { events };
            execute_repository_operation(repository_root, request, &mut reporter)
        })
    }
}

struct ChannelOperationReporter {
    events: async_channel::Sender<OperationEvent>,
}

impl OperationReporter for ChannelOperationReporter {
    fn report(&mut self, event: OperationEvent) {
        let _ = self.events.send_blocking(event);
    }
}

pub struct NativeBrowserService;

impl BrowserService for NativeBrowserService {
    fn open_url(&self, url: &str, cx: &mut App) -> Result<(), String> {
        let parsed = Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;
        match parsed.scheme() {
            "http" | "https" => {
                cx.open_url(url);
                Ok(())
            }
            scheme => Err(format!("Unsupported URL scheme: {scheme}")),
        }
    }
}

#[cfg(test)]
pub use test_support::{FakeOperationService, RecordingBrowserService};

#[cfg(test)]
mod test_support {
    use super::{BrowserService, OperationFuture, OperationService};
    use gpui::App;
    use stax::application::{
        OperationEvent, OperationOutcome, OperationProgress, OperationReceipt, OperationRequest,
        OperationResult, OperationStage,
    };
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use url::Url;

    #[derive(Default)]
    pub struct FakeOperationService {
        requests: Mutex<Vec<OperationRequest>>,
        pending: Mutex<VecDeque<async_channel::Sender<OperationResult>>>,
        progress_count: Mutex<usize>,
    }

    impl FakeOperationService {
        pub fn requests(&self) -> Vec<OperationRequest> {
            self.requests.lock().unwrap().clone()
        }

        pub fn script_progress_count(&self, count: usize) {
            *self.progress_count.lock().unwrap() = count;
        }

        pub fn script_pr_url(&self, branch: &str, url: &str) {
            let request = OperationRequest::ResolvePullRequestUrl {
                branch: branch.to_string(),
            };
            let receipt = OperationReceipt {
                request,
                summary: format!("Resolved pull request for {branch}"),
                affected_branches: vec![branch.to_string()],
                outcome: OperationOutcome::PullRequestResolved {
                    branch: branch.to_string(),
                    url: url.to_string(),
                },
                transaction: None,
                warnings: Vec::new(),
                side_effects: stax::application::OperationSideEffects::None,
            };
            self.complete_next_success(receipt);
        }

        pub fn complete_next_success(&self, receipt: OperationReceipt) {
            self.complete_next(Ok(receipt));
        }

        pub fn complete_next_error(&self, error: stax::application::OperationError) {
            self.complete_next(Err(error));
        }

        pub fn complete_submit_with_url(&self, url: &str) {
            let request = OperationRequest::SubmitStack {
                new_pull_requests: stax::application::PullRequestMode::Draft,
            };
            let receipt = OperationReceipt {
                request,
                summary: "Submitted stack".into(),
                affected_branches: vec!["child".into()],
                outcome: OperationOutcome::Submitted {
                    pull_requests: vec![stax::application::PullRequestReceipt {
                        branch: "child".into(),
                        number: 42,
                        url: url.into(),
                        change: stax::application::PullRequestChange::Updated,
                    }],
                },
                transaction: None,
                warnings: Vec::new(),
                side_effects: stax::application::OperationSideEffects::RemoteMayHaveChanged,
            };
            self.complete_next_success(receipt);
        }

        fn complete_next(&self, result: OperationResult) {
            let sender = self
                .pending
                .lock()
                .unwrap()
                .pop_front()
                .expect("no pending fake operation");
            sender
                .try_send(result)
                .expect("pending fake operation already completed");
        }
    }

    impl OperationService for FakeOperationService {
        fn execute(
            &self,
            _repository_root: PathBuf,
            request: OperationRequest,
            events: async_channel::Sender<OperationEvent>,
        ) -> OperationFuture {
            self.requests.lock().unwrap().push(request.clone());
            let progress_count = *self.progress_count.lock().unwrap();
            let (result_sender, result_receiver) = async_channel::bounded(1);
            self.pending.lock().unwrap().push_back(result_sender);
            Box::pin(async move {
                let _ = events.send(OperationEvent::Started(request)).await;
                let result = result_receiver
                    .recv()
                    .await
                    .expect("fake operation completion channel closed");
                for completed in 1..=progress_count {
                    let _ = events
                        .send(OperationEvent::Progress(OperationProgress {
                            stage: OperationStage::Preparing,
                            completed,
                            total: Some(progress_count),
                            branch: Some(format!("branch-{completed}")),
                            message: format!("step {completed}"),
                        }))
                        .await;
                }
                match &result {
                    Ok(receipt) => {
                        let _ = events
                            .send(OperationEvent::Completed(receipt.clone()))
                            .await;
                    }
                    Err(error) => {
                        let _ = events.send(OperationEvent::Failed(error.clone())).await;
                    }
                }
                result
            })
        }
    }

    #[derive(Default)]
    pub struct RecordingBrowserService {
        urls: Mutex<Vec<String>>,
    }

    impl RecordingBrowserService {
        pub fn urls(&self) -> Vec<String> {
            self.urls.lock().unwrap().clone()
        }
    }

    impl BrowserService for RecordingBrowserService {
        fn open_url(&self, url: &str, _cx: &mut App) -> Result<(), String> {
            let parsed = Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;
            match parsed.scheme() {
                "http" | "https" => {
                    self.urls.lock().unwrap().push(url.to_string());
                    Ok(())
                }
                scheme => Err(format!("Unsupported URL scheme: {scheme}")),
            }
        }
    }
}
