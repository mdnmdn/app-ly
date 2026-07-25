use tauri::State;

#[derive(Clone)]
pub struct KeychainState {
    pub prefix: String,
}

fn prefixed_service(prefix: &str, service: &str) -> String {
    if prefix.is_empty() {
        service.to_string()
    } else {
        format!("{prefix}/{service}")
    }
}

#[tauri::command]
pub fn shell_secret_set(
    state: State<KeychainState>,
    service: String,
    account: String,
    password: String,
) -> Result<(), String> {
    let prefixed = prefixed_service(&state.prefix, &service);
    println!("shell_secret_set: prefix={}, service={}, full={prefixed}", state.prefix, service);
    let entry =
        keyring::Entry::new(&prefixed, &account).map_err(|e| format!("keyring entry: {e}"))?;
    entry
        .set_password(&password)
        .map_err(|e| format!("set secret: {e}"))
}

#[tauri::command]
pub fn shell_secret_get(
    state: State<KeychainState>,
    service: String,
    account: String,
) -> Result<String, String> {
    let prefixed = prefixed_service(&state.prefix, &service);
    println!("shell_secret_get: prefix={}, service={}, full={prefixed}", state.prefix, service);
    let entry =
        keyring::Entry::new(&prefixed, &account).map_err(|e| format!("keyring entry: {e}"))?;
    entry
        .get_password()
        .map_err(|e| format!("get secret: {e}"))
}

#[tauri::command]
pub fn shell_secret_delete(
    state: State<KeychainState>,
    service: String,
    account: String,
) -> Result<(), String> {
    let prefixed = prefixed_service(&state.prefix, &service);
    println!("shell_secret_delete: prefix={}, service={}, full={prefixed}", state.prefix, service);
    let entry =
        keyring::Entry::new(&prefixed, &account).map_err(|e| format!("keyring entry: {e}"))?;
    entry
        .delete_credential()
        .map_err(|e| format!("delete secret: {e}"))
}
