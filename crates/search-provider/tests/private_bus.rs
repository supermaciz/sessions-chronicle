#![cfg(unix)]

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use sessions_chronicle_core::database::schema::initialize_database;
use sessions_chronicle_search_provider::config::{
    APP_ID, application_object_path, bus_name, provider_object_path,
};
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

    fn pid(&self) -> u32 {
        self.0.as_ref().expect("child is alive").id()
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

/// Wait for `pid` to own the provider's well-known name before any call is made.
///
/// Probing readiness with a provider method would defeat the purpose: a call to an
/// unowned name is exactly what makes D-Bus start the service. On a machine where the
/// app is installed, that starts the *installed* provider against the *real* database
/// instead of the child this test spawned against its fixture. `NameHasOwner` and
/// `GetConnectionUnixProcessID` never activate anything, so the wait stays inert and
/// the ownership assertion turns a silent hijack into a clear failure.
async fn proxy_owned_by(pid: u32) -> SearchProviderProxy<'static> {
    let connection = connection().await;
    let dbus = zbus::fdo::DBusProxy::new(connection).await.unwrap();
    let name = zbus::names::BusName::try_from(bus_name(APP_ID)).unwrap();
    for _ in 0..500 {
        if dbus.name_has_owner(name.clone()).await.unwrap_or(false) {
            let owner = dbus
                .get_connection_unix_process_id(name.clone())
                .await
                .unwrap_or(0);
            assert_eq!(
                owner,
                pid,
                "{} was taken by pid {owner}, not the provider this test spawned (pid {pid}); \
                 an installed D-Bus activation file is shadowing the fixture",
                bus_name(APP_ID)
            );
            return SearchProviderProxy::new(connection).await.unwrap();
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

#[derive(Debug)]
struct ActivationCall {
    action: String,
    parameters: Vec<String>,
    platform_data: HashMap<String, String>,
}

struct MockApplication {
    sender: async_channel::Sender<ActivationCall>,
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl MockApplication {
    async fn activate_action(
        &self,
        action: String,
        parameters: Vec<OwnedValue>,
        platform_data: HashMap<String, OwnedValue>,
    ) {
        let parameters = parameters
            .into_iter()
            .map(|value| value.downcast_ref::<Str>().unwrap().to_string())
            .collect();
        let platform_data = platform_data
            .into_iter()
            .map(|(key, value)| (key, value.downcast_ref::<Str>().unwrap().to_string()))
            .collect();
        let _ = self
            .sender
            .send(ActivationCall {
                action,
                parameters,
                platform_data,
            })
            .await;
    }
}

static BUS_NAME_LOCK: Mutex<()> = Mutex::new(());

async fn recv_activation_call(
    receiver: &async_channel::Receiver<ActivationCall>,
) -> ActivationCall {
    futures_lite::future::or(
        async { receiver.recv().await.expect("activation channel closed") },
        async {
            async_io::Timer::after(Duration::from_secs(5)).await;
            panic!("timed out waiting for ActivateAction call");
        },
    )
    .await
}

async fn mock_application() -> (zbus::Connection, async_channel::Receiver<ActivationCall>) {
    let (sender, receiver) = async_channel::unbounded();
    let connection = zbus::connection::Builder::session()
        .unwrap()
        .name(APP_ID)
        .unwrap()
        .serve_at(application_object_path(APP_ID), MockApplication { sender })
        .unwrap()
        .build()
        .await
        .unwrap();
    (connection, receiver)
}

#[test]
fn private_bus_provider_contract_and_fresh_process_metadata() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("skipping private-bus test: DBUS_SESSION_BUS_ADDRESS is absent");
        return;
    }
    let _lock = BUS_NAME_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    async_io::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("sessions.db");
        seed_database(&database);
        install_settings(&temp, false);

        let child = ChildGuard::new(provider(&database, &temp));
        let provider_proxy = proxy_owned_by(child.pid()).await;

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

        // Narrowing must never subtract from what the same terms return on their own:
        // Shell's previous set is capped, so intersecting with it would permanently drop
        // matches that fell outside the cap of an earlier, shorter query.
        let subsearch = provider_proxy
            .get_subsearch_result_set(vec!["outside".into(), "known".into()], vec!["aki".into()])
            .await
            .unwrap();
        assert_eq!(subsearch, ["known"]);

        let from_empty = provider_proxy
            .get_subsearch_result_set(Vec::new(), vec!["aki".into()])
            .await
            .unwrap();
        assert_eq!(from_empty, ["known"]);

        let from_unrelated = provider_proxy
            .get_subsearch_result_set(vec!["outside".into()], vec!["aki".into()])
            .await
            .unwrap();
        assert_eq!(from_unrelated, ["known"]);

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
        let excerpt_proxy = proxy_owned_by(excerpt_child.pid()).await;
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
        let fresh_proxy = proxy_owned_by(fresh_child.pid()).await;
        let fresh = fresh_proxy
            .get_result_metas(vec!["known".into()])
            .await
            .unwrap();
        assert_eq!(value(&fresh[0], "description"), hidden_description);
        drop(fresh_child);
    });
}

#[test]
fn activation_calls_application_action_with_expected_payloads() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("skipping private-bus test: DBUS_SESSION_BUS_ADDRESS is absent");
        return;
    }
    let _lock = BUS_NAME_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    async_io::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("sessions.db");
        seed_database(&database);
        install_settings(&temp, false);

        let (_app_connection, receiver) = mock_application().await;

        let child = ChildGuard::new(provider(&database, &temp));
        let provider_proxy = proxy_owned_by(child.pid()).await;

        provider_proxy
            .activate_result("session-a".into(), vec![], 42)
            .await
            .unwrap();
        let open = recv_activation_call(&receiver).await;
        assert_eq!(open.action, "open-session");
        assert_eq!(open.parameters, vec!["session-a".to_string()]);
        assert_eq!(open.platform_data["desktop-startup-id"], "_TIME42");

        provider_proxy
            .launch_search(vec!["alpha".into(), "beta".into()], 84)
            .await
            .unwrap();
        let search = recv_activation_call(&receiver).await;
        assert_eq!(search.action, "search-sessions");
        assert_eq!(search.parameters, vec!["alpha beta".to_string()]);
        assert_eq!(search.platform_data["desktop-startup-id"], "_TIME84");

        drop(child);
    });
}
