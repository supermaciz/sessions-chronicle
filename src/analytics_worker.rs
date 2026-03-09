use std::path::PathBuf;

use relm4::{ComponentSender, Worker};

use crate::database::analytics::load_analytics;
use crate::models::AnalyticsData;

pub struct AnalyticsWorker {
    db_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyticsWorkerInput {
    Load,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyticsWorkerOutput {
    Loaded(AnalyticsData),
    Failed(String),
}

impl Worker for AnalyticsWorker {
    type Init = PathBuf;
    type Input = AnalyticsWorkerInput;
    type Output = AnalyticsWorkerOutput;

    fn init(init: Self::Init, _sender: ComponentSender<Self>) -> Self {
        Self { db_path: init }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
        match message {
            AnalyticsWorkerInput::Load => match load_analytics(&self.db_path) {
                Ok(data) => {
                    let _ = sender.output(AnalyticsWorkerOutput::Loaded(data));
                }
                Err(err) => {
                    tracing::error!("Analytics worker failed: {}", err);
                    let _ = sender.output(AnalyticsWorkerOutput::Failed(format!("{err:#}")));
                }
            },
        }
    }
}
