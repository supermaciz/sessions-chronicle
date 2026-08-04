use std::collections::HashMap;

use zbus::zvariant::{Str, Value};

use crate::config::application_object_path;

pub async fn activate_application_action(
    connection: &zbus::Connection,
    app_id: &str,
    action: &str,
    parameter: &str,
    timestamp: u32,
) -> zbus::Result<()> {
    let proxy = zbus::Proxy::new(
        connection,
        app_id,
        application_object_path(app_id),
        "org.freedesktop.Application",
    )
    .await?;
    let parameters = vec![Value::from(Str::from(parameter.to_owned()))];
    let platform_data = HashMap::from([(
        "desktop-startup-id",
        Value::from(Str::from(format!("_TIME{timestamp}"))),
    )]);
    proxy
        .call_method("ActivateAction", &(action, parameters, platform_data))
        .await?;
    Ok(())
}
