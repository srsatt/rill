fn validate_metadata(metadata: &PluginMetadata) -> Result<(), PluginError> {
    if metadata.id.trim().is_empty()
        || metadata.name.trim().is_empty()
        || metadata.version.trim().is_empty()
        || metadata.id.len() > 128
        || metadata.name.len() > 128
        || metadata.version.len() > 64
        || metadata.description.len() > 2_000
        || metadata.requested_permissions.len() > 32
    {
        return Err(PluginError::Invalid("metadata is out of range".into()));
    }
    if metadata.requested_permissions.iter().any(|permission| {
        permission != "http"
            && permission
                .strip_prefix("secret:")
                .is_none_or(|name| name.is_empty() || name.len() > 64)
    }) {
        return Err(PluginError::Invalid("unknown requested permission".into()));
    }
    Ok(())
}

fn validate_permission(permission: &PluginPermission) -> Result<(), PluginError> {
    if permission.capability == "http" {
        let hosts = permission
            .constraint
            .get("hosts")
            .and_then(Value::as_array)
            .ok_or_else(|| PluginError::Invalid("HTTP hosts constraint is required".into()))?;
        if hosts.is_empty()
            || hosts.len() > 32
            || hosts.iter().any(|host| {
                host.as_str().is_none_or(|host| {
                    host.is_empty() || host.len() > 253 || host.contains('/') || host.contains('@')
                })
            })
        {
            return Err(PluginError::Invalid("invalid HTTP hosts constraint".into()));
        }
    } else if permission.capability.starts_with("secret:") {
        if permission
            .constraint
            .get("secretId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(PluginError::Invalid(
                "secretId constraint is required".into(),
            ));
        }
    } else {
        return Err(PluginError::Invalid("unknown capability".into()));
    }
    Ok(())
}

fn trap(error: impl std::fmt::Display) -> PluginError {
    PluginError::Trap(error.to_string())
}

fn to_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

const fn default_poll_seconds() -> u64 {
    900
}

const fn enabled() -> bool {
    true
}

