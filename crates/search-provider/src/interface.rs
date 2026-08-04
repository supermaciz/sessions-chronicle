use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sessions_chronicle_core::database::shell_search::ShellSearchMetadata;
use zbus::zvariant::{OwnedValue, Str};

use crate::config::application_object_path;
use crate::db_worker::DbWorker;
use crate::lifecycle::{ActivityTracker, CallGuard};

const APPLICATION_ICON: &str = "dev.maciz.sessionschronicle";

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
            OwnedValue::from(Str::from(APPLICATION_ICON)),
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
            OwnedValue::from(Str::from(APPLICATION_ICON)),
        );
        result
    }

    async fn activate(&self, action: &str, parameter: String, timestamp: u32) {
        let connection = match zbus::Connection::session().await {
            Ok(connection) => connection,
            Err(error) => {
                tracing::debug!(%error, "could not connect to application for search action");
                return;
            }
        };
        let platform_data: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::from([(
            "desktop-startup-id",
            zbus::zvariant::Value::from(format!("_TIME{timestamp}")),
        )]);
        let parameters = (
            action,
            vec![zbus::zvariant::Value::from(parameter)],
            platform_data,
        );
        if let Err(error) = connection
            .call_method(
                Some(self.app_id.as_str()),
                application_object_path(&self.app_id),
                Some("org.freedesktop.Application"),
                "ActivateAction",
                &parameters,
            )
            .await
        {
            tracing::debug!(%error, "application search action failed quietly");
        }
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

    async fn activate_result(&self, identifier: String, terms: Vec<String>, timestamp: u32) {
        let _guard = self.guard();
        let _ = terms;
        self.activate("open-session", identifier, timestamp).await;
    }

    async fn launch_search(&self, terms: Vec<String>, timestamp: u32) {
        let _guard = self.guard();
        self.activate("search-sessions", terms.join(" "), timestamp)
            .await;
    }
}
