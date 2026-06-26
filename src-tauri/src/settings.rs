use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppSettings {
    #[serde(default)]
    pub tools: BTreeMap<String, bool>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path).map_err(|error| format!("读取设置失败: {error}"))?;
        serde_json::from_str(&raw).map_err(|error| format!("解析设置失败: {error}"))
    }

    pub fn save(path: &Path, settings: &Self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(settings)
            .map_err(|error| format!("序列化设置失败: {error}"))?;
        fs::write(path, content).map_err(|error| format!("保存设置失败: {error}"))
    }
}
