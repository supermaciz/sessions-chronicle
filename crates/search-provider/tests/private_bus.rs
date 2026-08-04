#![cfg(unix)]

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use rusqlite::Connection;
use sessions_chronicle_core::database::schema::initialize_database;
use sessions_chronicle_search_provider::config::{APP_ID, bus_name, provider_object_path};
use tempfile::TempDir;
use zbus::zvariant::{OwnedValue, Str};

#[zbus::proxy(
    interface = "org.gnome.Shell.SearchProvider2",
    default_service = "dev.maciz.sessionschronicle.Devel.SearchProvider",
    default_path = "/dev/maciz/sessionschronicle/Devel/SearchProvider"
)]
trait SearchProvider {
    fn get_initial_result_set(&self, terms: Vec<String>) -> zbus::Result<Vec<String>>;
    fn get_subsearch_result_set(
        &self,
        previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> zbus::Result<Vec<String>>;
    fn get_result_metas(
        &self,
        identifiers: Vec<String>,
    ) -> zbus::Result<Vec<HashMap<String, OwnedValue>>>;
    fn activate_result(
        &self,
        identifier: String,
        terms: Vec<String>,
        timestamp: u32,
    ) -> zbus::Result<()>;
    fn launch_search(&self, terms: Vec<String>, timestamp: u32) -> zbus::Result<()>;
}

fn seed_database(path: &Path) {
    let connection = Connection::open(path).unwrap();
    initialize_database(&connection).unwrap();
    connection
        .execute(
            "INSERT INTO sessions (
                 id, first_prompt, tool, start_time, message_count, file_path,
                 last_updated, is_subagent
             ) VALUES ('known', 'Private bus prompt', 'claude_code', 1700000000,
                       1, '/known.jsonl', 1700000000, 0)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO messages (
                 session_id, message_index, role, content, timestamp
             ) VALUES ('known', 0, 'user', 'aki transcript secret', 1700000000)",
            [],
        )
        .unwrap();
}

fn install_settings(temp: &TempDir, show_excerpts: bool) {
    let schema_dir = temp.path().join("schemas");
    fs::create_dir_all(&schema_dir).unwrap();
    fs::write(
        schema_dir.join("org.gnome.SessionsChronicle.gschema.xml"),
        format!(
            r#"<schemalist>
  <schema id="dev.maciz.sessionschronicle.Devel" path="/dev/maciz/sessionschronicle/Devel/">
    <key name="search-provider-show-excerpts" type="b">
      <default>{show_excerpts}</default>
    </key>
  </schema>
</schemalist>
"#
        ),
    )
    .unwrap();
    Command::new("glib-compile-schemas")
        .arg(&schema_dir)
        .status()
        .unwrap()
        .success()
        .then_some(())
        .expect("glib-compile-schemas");

    let config_dir = temp.path().join("config").join("glib-2.0").join("settings");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("keyfile"),
        format!(
            "[{}]\nsearch-provider-show-excerpts={}\n",
            APP_ID, show_excerpts
        ),
    )
    .unwrap();
}

fn provider(database: &Path, settings: &TempDir) -> Child {
    Command::new(env!("CARGO_BIN_EXE_sessions-chronicle-search-provider"))
        .arg("--database")
        .arg(database)
        .env("GSETTINGS_SCHEMA_DIR", settings.path().join("schemas"))
        .env("GSETTINGS_BACKEND", "keyfile")
        .env("XDG_CONFIG_HOME", settings.path().join("config"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn connection() -> &'static zbus::Connection {
    let connection = zbus::Connection::session().await.unwrap();
    Box::leak(Box::new(connection))
}

async fn proxy() -> SearchProviderProxy<'static> {
    let connection = connection().await;
    for _ in 0..100 {
        if let Ok(proxy) = SearchProviderProxy::new(connection).await {
            if proxy
                .get_initial_result_set(vec!["ak".into()])
                .await
                .is_ok()
            {
                return proxy;
            }
        }
        async_io::Timer::after(Duration::from_millis(10)).await;
    }
    panic!("provider did not acquire {}", bus_name(APP_ID));
}

fn value(meta: &HashMap<String, OwnedValue>, key: &str) -> String {
    meta.get(key)
        .unwrap()
        .downcast_ref::<Str>()
        .unwrap()
        .to_string()
}

#[test]
fn private_bus_provider_contract_and_fresh_process_metadata() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("skipping private-bus test: DBUS_SESSION_BUS_ADDRESS is absent");
        return;
    }

    async_io::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("sessions.db");
        seed_database(&database);
        install_settings(&temp, false);

        let child = ChildGuard::new(provider(&database, &temp));
        let provider_proxy = proxy().await;

        let xml: String = connection()
            .await
            .call_method(
                Some(bus_name(APP_ID)),
                provider_object_path(APP_ID),
                Some("org.freedesktop.DBus.Introspectable"),
                "Introspect",
                &(),
            )
            .await
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        for (name, input, output) in [
            (
                "GetInitialResultSet",
                "<arg name=\"terms\" type=\"as\" direction=\"in\"/>",
                "<arg type=\"as\" direction=\"out\"/>",
            ),
            (
                "GetSubsearchResultSet",
                "<arg name=\"previous_results\" type=\"as\" direction=\"in\"/>",
                "<arg type=\"as\" direction=\"out\"/>",
            ),
            (
                "GetResultMetas",
                "<arg name=\"identifiers\" type=\"as\" direction=\"in\"/>",
                "<arg type=\"aa{sv}\" direction=\"out\"/>",
            ),
            (
                "ActivateResult",
                "<arg name=\"identifier\" type=\"s\" direction=\"in\"/>",
                "<arg name=\"timestamp\" type=\"u\" direction=\"in\"/>",
            ),
            (
                "LaunchSearch",
                "<arg name=\"terms\" type=\"as\" direction=\"in\"/>",
                "<arg name=\"timestamp\" type=\"u\" direction=\"in\"/>",
            ),
        ] {
            assert!(xml.contains(&format!("name=\"{name}\"")));
            assert!(xml.contains(input));
            assert!(xml.contains(output));
        }

        assert!(
            provider_proxy
                .get_initial_result_set(vec!["ak".into()])
                .await
                .unwrap()
                .is_empty()
        );
        let ids = provider_proxy
            .get_initial_result_set(vec!["aki".into()])
            .await
            .unwrap();
        assert_eq!(ids, ["known"]);

        let subsearch = provider_proxy
            .get_subsearch_result_set(vec!["outside".into(), "known".into()], vec!["aki".into()])
            .await
            .unwrap();
        assert_eq!(subsearch, ["known"]);

        let hidden = provider_proxy
            .get_result_metas(vec!["known".into(), "missing".into(), "known".into()])
            .await
            .unwrap();
        assert_eq!(hidden.len(), 3);
        assert_eq!(value(&hidden[0], "id"), "known");
        assert_eq!(value(&hidden[1], "id"), "missing");
        assert_eq!(value(&hidden[1], "name"), "Session unavailable");
        assert_eq!(value(&hidden[1], "gicon"), APP_ID);
        assert_eq!(value(&hidden[2], "id"), "known");
        assert_eq!(value(&hidden[2], "gicon"), APP_ID);
        let hidden_description = value(&hidden[0], "description");
        assert_eq!(value(&hidden[0], "gicon"), APP_ID);

        drop(child);

        install_settings(&temp, true);
        let excerpt_child = ChildGuard::new(provider(&database, &temp));
        let excerpt_proxy = proxy().await;
        assert_eq!(
            excerpt_proxy
                .get_initial_result_set(vec!["aki".into()])
                .await
                .unwrap(),
            ["known"]
        );
        let shown = excerpt_proxy
            .get_result_metas(vec!["known".into()])
            .await
            .unwrap();
        assert_ne!(value(&shown[0], "description"), hidden_description);

        drop(excerpt_child);

        install_settings(&temp, true);
        let fresh_child = ChildGuard::new(provider(&database, &temp));
        let fresh_proxy = proxy().await;
        let fresh = fresh_proxy
            .get_result_metas(vec!["known".into()])
            .await
            .unwrap();
        assert_eq!(value(&fresh[0], "description"), hidden_description);
        drop(fresh_child);
    });
}
