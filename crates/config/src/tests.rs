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
}
