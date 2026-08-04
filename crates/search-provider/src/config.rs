pub const APP_ID: &str = match option_env!("SESSIONS_CHRONICLE_APP_ID") {
    Some(value) => value,
    None => "dev.maciz.sessionschronicle.Devel",
};

pub fn bus_name(app_id: &str) -> String {
    format!("{app_id}.SearchProvider")
}

pub fn provider_object_path(app_id: &str) -> String {
    format!("/{}/SearchProvider", app_id.replace('.', "/"))
}

pub fn application_object_path(app_id: &str) -> String {
    format!("/{}", app_id.replace('.', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_names_form_one_activation_chain() {
        let app_id = "dev.maciz.sessionschronicle.Devel";
        assert_eq!(
            bus_name(app_id),
            "dev.maciz.sessionschronicle.Devel.SearchProvider"
        );
        assert_eq!(
            provider_object_path(app_id),
            "/dev/maciz/sessionschronicle/Devel/SearchProvider"
        );
        assert_eq!(
            application_object_path(app_id),
            "/dev/maciz/sessionschronicle/Devel"
        );
    }
}
