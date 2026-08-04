use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sessions_chronicle_core::database::shell_search::ShellSearchMetadata;
use zbus::zvariant::{OwnedValue, Str};

use crate::activation::activate_application_action;
use crate::db_worker::DbWorker;
use crate::lifecycle::{ActivityTracker, CallGuard};

#[derive(Clone)]
pub struct SearchProvider {
    worker: Arc<DbWorker>,
    app_id: String,
    activity: ActivityTracker,
    latest_expression: Arc<Mutex<Option<String>>>,
}

impl SearchProvider {
    pub fn new(worker: Arc<DbWorker>, app_id: String, activity: ActivityTracker) -> Self {
        Self {
            worker,
            app_id,
            activity,
            latest_expression: Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.activity.is_idle()
    }

    fn guard(&self) -> CallGuard {
        self.activity.enter()
    }

    fn expression(&self) -> Option<String> {
        self.latest_expression.lock().unwrap().clone()
    }

    fn set_expression(&self, expression: Option<String>) {
        *self.latest_expression.lock().unwrap() = expression;
    }

    fn unavailable(&self, id: &str) -> HashMap<String, OwnedValue> {
        let mut metadata = HashMap::new();
        metadata.insert("id".into(), OwnedValue::from(Str::from(id.to_owned())));
        metadata.insert(
            "name".into(),
            OwnedValue::from(Str::from("Session unavailable")),
        );
        metadata.insert(
            "gicon".into(),
            OwnedValue::from(Str::from(self.app_id.clone())),
        );
        metadata
    }

    fn render(
        &self,
        metadata: &ShellSearchMetadata,
        show_excerpts: bool,
    ) -> HashMap<String, OwnedValue> {
        let rendered = metadata.render(chrono::Utc::now(), show_excerpts);
        let mut result = HashMap::new();
        result.insert(
            "id".into(),
            OwnedValue::from(Str::from(metadata.id.clone())),
        );
        result.insert("name".into(), OwnedValue::from(Str::from(rendered.name)));
        result.insert(
            "description".into(),
            OwnedValue::from(Str::from(rendered.description)),
        );
        result.insert(
            "gicon".into(),
            OwnedValue::from(Str::from(self.app_id.clone())),
        );
        result
    }
}

#[zbus::interface(name = "org.gnome.Shell.SearchProvider2", spawn = true)]
impl SearchProvider {
    async fn get_initial_result_set(&self, terms: Vec<String>) -> Vec<String> {
        let _guard = self.guard();
        let response = self.worker.search_terms(&terms, None).await;
        self.set_expression(response.expression);
        response.ids
    }

    async fn get_subsearch_result_set(
        &self,
        previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> Vec<String> {
        let _guard = self.guard();
        let response = self
            .worker
            .search_terms(&terms, Some(previous_results))
            .await;
        self.set_expression(response.expression);
        response.ids
    }

    async fn get_result_metas(&self, identifiers: Vec<String>) -> Vec<HashMap<String, OwnedValue>> {
        let _guard = self.guard();
        let response = self
            .worker
            .metadata(identifiers.clone(), self.expression())
            .await;
        response
            .rows
            .into_iter()
            .zip(identifiers)
            .map(|(row, id)| match row {
                Some(metadata) => self.render(&metadata, response.show_excerpts),
                None => self.unavailable(&id),
            })
            .collect()
    }

    #[zbus(name = "ActivateResult")]
    async fn activate_result(
        &self,
        identifier: String,
        _terms: Vec<String>,
        timestamp: u32,
        #[zbus(connection)] connection: &zbus::Connection,
    ) {
        let _guard = self.guard();
        if let Err(error) = activate_application_action(
            connection,
            &self.app_id,
            "open-session",
            &identifier,
            timestamp,
        )
        .await
        {
            tracing::warn!(%error, "failed to activate Shell search result");
        }
    }

    #[zbus(name = "LaunchSearch")]
    async fn launch_search(
        &self,
        terms: Vec<String>,
        timestamp: u32,
        #[zbus(connection)] connection: &zbus::Connection,
    ) {
        let _guard = self.guard();
        let query = terms.join(" ");
        if let Err(error) = activate_application_action(
            connection,
            &self.app_id,
            "search-sessions",
            &query,
            timestamp,
        )
        .await
        {
            tracing::warn!(%error, "failed to launch carried Shell search");
        }
    }
}
