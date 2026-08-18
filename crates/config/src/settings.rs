impl Settings {
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let mut settings = match path {
            Some(path) => {
                let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                })?;
                toml::from_str(&text).map_err(|source| ConfigError::Parse {
                    path: path.to_path_buf(),
                    source,
                })?
            }
            None => Self::default(),
        };
        settings.apply_environment();
        settings.validate()?;
        Ok(settings)
    }

    fn apply_environment(&mut self) {
        if let Some(value) = env::var_os("RILL_BIND") {
            self.http.bind = value.to_string_lossy().into_owned();
        }
        if let Some(value) = env::var_os("RILL_PUBLIC_BASE_URL") {
            self.http.public_base_url = value.to_string_lossy().into_owned();
        }
        if let Some(value) = env::var_os("RILL_DATABASE_PATH") {
            self.database.path = PathBuf::from(value);
        }
        if let Some(value) = env::var_os("RILL_STATIC_DIR") {
            self.assets.static_dir = PathBuf::from(value);
        }
        if let Some(value) = env::var_os("RILL_RENDERER_WASM") {
            self.assets.renderer_wasm = PathBuf::from(value);
        }
        if let Some(value) = env::var_os("RILL_LOG") {
            self.logging.filter = value.to_string_lossy().into_owned();
        }
        if let Some(value) = env::var_os("RILL_METRICS_PATH") {
            self.metrics.path = value.to_string_lossy().into_owned();
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.http.bind.parse::<SocketAddr>().map_err(|error| {
            ConfigError::Invalid(format!("http.bind must be an IP socket address: {error}"))
        })?;
        let public_url = Url::parse(&self.http.public_base_url).map_err(|error| {
            ConfigError::Invalid(format!(
                "http.public_base_url must be an absolute URL: {error}"
            ))
        })?;
        if !matches!(public_url.scheme(), "http" | "https") || public_url.host_str().is_none() {
            return Err(ConfigError::Invalid(
                "http.public_base_url must use http or https and include a host".to_owned(),
            ));
        }
        if self.database.pool_size == 0 || self.database.pool_size > 32 {
            return Err(ConfigError::Invalid(
                "database.pool_size must be between 1 and 32".to_owned(),
            ));
        }
        if self.database.busy_timeout_ms == 0 {
            return Err(ConfigError::Invalid(
                "database.busy_timeout_ms must be greater than zero".to_owned(),
            ));
        }
        if self.auth.session_days == 0 || self.auth.reader_session_days == 0 {
            return Err(ConfigError::Invalid(
                "auth session lifetimes must be greater than zero".to_owned(),
            ));
        }
        if self.auth.pairing_minutes == 0 || self.auth.pairing_max_attempts == 0 {
            return Err(ConfigError::Invalid(
                "pairing lifetime and maximum attempts must be greater than zero".to_owned(),
            ));
        }
        if self.renderer.max_response_bytes < 1024 || self.renderer.memory_bytes < 1_048_576 {
            return Err(ConfigError::Invalid(
                "renderer limits are too small to serve a page".to_owned(),
            ));
        }
        if self.fetch.timeout_seconds == 0 || self.fetch.max_response_bytes < 1024 {
            return Err(ConfigError::Invalid(
                "fetch timeout and response limit must be greater than zero".to_owned(),
            ));
        }
        if self.fetch.max_redirects > 20 {
            return Err(ConfigError::Invalid(
                "fetch.max_redirects must not exceed 20".to_owned(),
            ));
        }
        if self.ingestion.maximum_collection_fan_out == 0
            || self.ingestion.maximum_collection_fan_out > 200
        {
            return Err(ConfigError::Invalid(
                "ingestion.maximum_collection_fan_out must be between 1 and 200".to_owned(),
            ));
        }
        if !self.ingestion.collection_detection_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.ingestion.collection_detection_threshold)
        {
            return Err(ConfigError::Invalid(
                "ingestion.collection_detection_threshold must be between 0 and 1".to_owned(),
            ));
        }
        if !matches!(
            self.ingestion.collection_parent_display_default.as_str(),
            "children_only" | "parent_and_children" | "parent_only"
        ) {
            return Err(ConfigError::Invalid(
                "ingestion.collection_parent_display_default is invalid".to_owned(),
            ));
        }
        if self.jobs.concurrency == 0 || self.jobs.concurrency > 32 {
            return Err(ConfigError::Invalid(
                "jobs.concurrency must be between 1 and 32".to_owned(),
            ));
        }
        if self.recommendations.refit_batch_size == 0
            || self.recommendations.fit_window < self.recommendations.refit_batch_size
            || self.recommendations.fit_window > 10_000
        {
            return Err(ConfigError::Invalid(
                "recommendation refit batch/window limits are invalid".to_owned(),
            ));
        }
        if self.secrets.master_key_env.trim().is_empty() || self.secrets.key_version < 1 {
            return Err(ConfigError::Invalid(
                "secrets master key environment name and key version are invalid".to_owned(),
            ));
        }
        if self.email.timeout_seconds == 0 || self.email.maximum_message_bytes < 1024 {
            return Err(ConfigError::Invalid(
                "email timeout and message limit must be greater than zero".to_owned(),
            ));
        }
        for (name, provider) in [
            ("embedding", self.models.embedding.as_ref()),
            ("summary", self.models.summary.as_ref()),
            ("recommendation", self.models.recommendation.as_ref()),
            ("collection_parser", self.models.collection_parser.as_ref()),
        ] {
            if let Some(provider) = provider {
                provider.validate(name)?;
            }
        }
        if self.plugins.memory_bytes < 1_048_576
            || self.plugins.fuel == 0
            || self.plugins.timeout_ms == 0
            || self.plugins.maximum_output_bytes < 1024
            || self.plugins.maximum_component_bytes < 1024
        {
            return Err(ConfigError::Invalid(
                "plugin resource limits are too small".to_owned(),
            ));
        }
        if !self.metrics.path.starts_with('/')
            || self.metrics.path.len() > 128
            || self.metrics.path.contains(['{', '}', '*'])
        {
            return Err(ConfigError::Invalid(
                "metrics.path must be a fixed absolute HTTP path".to_owned(),
            ));
        }
        Ok(())
    }
}
