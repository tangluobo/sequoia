//! Reads the existing global.json names and original encrypted key format.
use super::trim;
use crate::preferences::Preferences;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub base: String,
    pub model: String,
    pub key: String,
    pub include_schema: bool,
    pub timeout: u64,
}
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("configured", &self.configured())
            .field("include_schema", &self.include_schema)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}
impl Settings {
    pub fn read(preferences: &Preferences) -> Self {
        let text = |name: &str, default: &str| {
            preferences
                .extra
                .get(name)
                .map_or(default, |value| value.as_str().unwrap_or_default())
                .to_owned()
        };
        let stored = text("aiApiKey", "");
        let key = if super::blank(&stored) {
            String::new()
        } else if let Some(encrypted) = stored.strip_prefix("TOMATO_ENCRYPTED:") {
            crate::legacy::decrypt(encrypted).unwrap_or_default()
        } else {
            stored
        };
        let timeout = preferences
            .extra
            .get("aiRequestTimeoutSeconds")
            .and_then(serde_json::Value::as_i64)
            .filter(|value| *value > 0)
            .unwrap_or(60) as u64;
        Self {
            base: trim(&text("aiApiBaseUrl", "https://api.openai.com/v1")).into(),
            model: trim(&text("aiModel", "")).into(),
            key,
            include_schema: preferences
                .extra
                .get("aiIncludeSchema")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            timeout,
        }
    }
    pub fn configured(&self) -> bool {
        !super::blank(&self.base) && !super::blank(&self.model)
    }
    pub fn store(&self, preferences: &mut Preferences) {
        for (name, value) in [
            ("aiApiBaseUrl", trim(&self.base)),
            ("aiModel", trim(&self.model)),
            ("aiApiKey", trim(&self.key)),
        ] {
            preferences.extra.insert(
                name.into(),
                serde_json::Value::String(if name == "aiApiKey" && !value.is_empty() {
                    format!(
                        "TOMATO_ENCRYPTED:{}",
                        crate::legacy::encrypt_text(value.as_bytes())
                    )
                } else {
                    value.into()
                }),
            );
        }
        preferences
            .extra
            .insert("aiIncludeSchema".into(), self.include_schema.into());
        preferences.extra.insert(
            "aiRequestTimeoutSeconds".into(),
            self.timeout.clamp(5, 300).into(),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn assert_original(settings: &Settings, expected: &serde_json::Value) {
        assert_eq!(settings.base, expected["base"].as_str().unwrap());
        assert_eq!(settings.model, expected["model"].as_str().unwrap());
        assert_eq!(settings.key, expected["key"].as_str().unwrap());
        assert_eq!(settings.timeout, expected["timeout"].as_u64().unwrap());
        assert_eq!(
            settings.include_schema,
            expected["includeSchema"].as_bool().unwrap()
        );
        assert_eq!(
            settings.configured(),
            expected["configured"].as_bool().unwrap()
        );
    }
    #[test]
    fn old_global_settings_and_encrypted_keys_match_actual_java() -> anyhow::Result<()> {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/sql-ai/original-settings.json"
        ))?;
        for case in source["loads"].as_array().unwrap() {
            let preferences = serde_json::from_value(case["input"].clone())?;
            assert_original(&Settings::read(&preferences), &case["output"]);
        }
        for case in source["saves"].as_array().unwrap() {
            let expected = &case["output"];
            let settings = Settings {
                base: " http://fixture.invalid/v1 ".into(),
                model: " fixture ".into(),
                key: case["key"].as_str().unwrap_or_default().into(),
                include_schema: false,
                timeout: case["timeout"].as_i64().unwrap().max(0) as u64,
            };
            let mut preferences = Preferences::default();
            preferences
                .extra
                .insert("future".into(), serde_json::json!({"preserve":42}));
            settings.store(&mut preferences);
            assert_eq!(preferences.extra["aiApiKey"], case["storedKey"]);
            assert_eq!(preferences.extra["future"]["preserve"], 42);
            assert_original(&Settings::read(&preferences), expected);
        }
        Ok(())
    }
}
