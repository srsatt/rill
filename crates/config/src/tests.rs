#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_pool_size() {
        let settings: Settings = toml::from_str("[database]\npool_size = 0").unwrap();
        assert!(
            settings
                .validate()
                .unwrap_err()
                .to_string()
                .contains("pool_size")
        );
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Settings>("mystery = true").is_err());
    }

    #[test]
    fn custom_model_ca_requires_https() {
        let settings: Settings = toml::from_str(
            r#"[models.summary]
base_url = "http://127.0.0.1:11434/v1/"
ca_certificate_path = "/etc/rill/model-ca.crt"
"#,
        )
        .unwrap();

        assert!(settings.validate().unwrap_err().to_string().contains("ca_certificate_path"));
    }

    #[test]
    fn trusted_origins_reject_paths() {
        let settings: Settings = toml::from_str(
            "[http]\ntrusted_origins = [\"https://rill.example/path\"]",
        )
        .unwrap();

        assert!(settings.validate().unwrap_err().to_string().contains("trusted_origins"));
    }
}
