impl PluginService {
    pub async fn inspect(&self, bytes: &[u8]) -> Result<PluginInspection, PluginError> {
        if bytes.is_empty() || bytes.len() > self.limits.maximum_component_bytes {
            return Err(PluginError::Invalid(
                "component size is out of range".into(),
            ));
        }
        let component = Component::new(&self.engine, bytes)
            .map_err(|error| PluginError::Invalid(error.to_string()))?;
        let (metadata, config_schema) = self.inspect_component(&component).await?;
        validate_metadata(&metadata)?;
        Ok(PluginInspection {
            metadata,
            component_sha256: format!("{:x}", Sha256::digest(bytes)),
            config_schema,
        })
    }
}
