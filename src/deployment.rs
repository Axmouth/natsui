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

// Account labels reached at $JS.<label>.API, typically through an administrator account's
// prefixed service imports of each account's JetStream API.
pub fn accounts() -> Result<Vec<String>, String> {
    parse_accounts(&std::env::var("NATSUI_ACCOUNTS").unwrap_or_default())
}
pub fn parse_accounts(value: &str) -> Result<Vec<String>, String> {
    let mut accounts: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
        .collect();
    if accounts.len() > 32 || accounts.iter().any(|a| !crate::telemetry::valid_token(a)) {
        return Err("NATSUI_ACCOUNTS must list up to 32 single-token account labels".into());
    }
    accounts.sort();
    accounts.dedup();
    Ok(accounts)
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

#[cfg(test)]
mod tests {
    #[test]
    fn account_lists_are_single_tokens() {
        assert_eq!(
            super::parse_accounts(" system2, system1,,system1 ").unwrap(),
            ["system1", "system2"]
        );
        assert!(super::parse_accounts("bad.name").is_err());
        assert!(super::parse_accounts("a b").is_err());
        assert!(super::parse_accounts("").unwrap().is_empty());
    }
}
