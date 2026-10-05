pub fn flag(name: &str) -> Result<bool, String> {
    match std::env::var(name).as_deref() {
        Ok("1") => Ok(true),
        Err(std::env::VarError::NotPresent) | Ok("0") => Ok(false),
        _ => Err(format!("{name} must be 0 or 1")),
    }
}

pub fn port() -> Result<u16, String> {
    std::env::var("NATSUI_PORT")
        .unwrap_or("4321".into())
        .parse()
        .map_err(|_| "NATSUI_PORT must be an integer between 0 and 65535".into())
}

pub fn domain() -> Result<String, String> {
    let domain = std::env::var("NATSUI_DOMAIN").unwrap_or_default();
    if !domain.is_empty() && !crate::telemetry::valid_token(&domain) {
        return Err("Invalid JetStream domain".into());
    }
    Ok(domain)
}

pub fn history_budget() -> Result<usize, String> {
    let mb: usize = std::env::var("NATSUI_HISTORY_MAX_MB")
        .unwrap_or("128".into())
        .parse()
        .map_err(|_| "NATSUI_HISTORY_MAX_MB must be an integer")?;
    if !(16..=4096).contains(&mb) {
        return Err("NATSUI_HISTORY_MAX_MB must be between 16 and 4096".into());
    }
    Ok(mb * 1024 * 1024)
}
