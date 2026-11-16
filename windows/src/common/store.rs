//! Réglages et petits fichiers JSON dans ~/.config/<App>/. Écriture atomique :
//! un fichier temporaire renommé, pour qu'un arrêt brutal ne laisse jamais un
//! JSON coupé en deux.

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Le dossier de données de l'app (~/.config/<App>), créé au besoin.
/// `override_var` permet aux tests de pointer ailleurs.
pub fn support_dir(app: &str, override_var: Option<&str>) -> PathBuf {
    if let Some(var) = override_var {
        if let Ok(p) = std::env::var(var) {
            if !p.is_empty() {
                let p = PathBuf::from(p);
                let _ = fs::create_dir_all(&p);
                return p;
            }
        }
    }
    let base = dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    let p = base.join(app);
    let _ = fs::create_dir_all(&p);
    p
}

pub fn cache_dir(app: &str) -> PathBuf {
    let base = dirs::cache_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".cache"));
    let p = base.join(app);
    let _ = fs::create_dir_all(&p);
    p
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let data = fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Écrit `value` en JSON ; avec `backup`, l'ancien fichier est d'abord copié en
/// `<nom>.backup.json`.
pub fn write_json<T: Serialize>(path: &Path, value: &T, backup: bool) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if backup && path.exists() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("data.json");
        let bak = path.with_file_name(name.replace(".json", ".backup.json"));
        let _ = fs::copy(path, bak);
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, serde_json::to_vec(value)?)?;
    fs::rename(&tmp, path)
}

/// Réglages clé → valeur, gardés en mémoire et écrits à chaque changement.
pub struct Settings {
    path: PathBuf,
    data: Map<String, Value>,
    defaults: Map<String, Value>,
}

impl Settings {
    pub fn new(path: PathBuf, defaults: Value) -> Self {
        let data = read_json::<Map<String, Value>>(&path).unwrap_or_default();
        let defaults = defaults.as_object().cloned().unwrap_or_default();
        Settings { path, data, defaults }
    }

    pub fn get(&self, key: &str) -> Value {
        self.data.get(key).or_else(|| self.defaults.get(key)).cloned().unwrap_or(Value::Null)
    }

    pub fn get_as<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_value(self.get(key)).ok()
    }

    pub fn bool(&self, key: &str) -> bool { self.get(key).as_bool().unwrap_or(false) }
    pub fn string(&self, key: &str) -> String { self.get(key).as_str().unwrap_or("").to_string() }
    pub fn f64(&self, key: &str) -> f64 { self.get(key).as_f64().unwrap_or(0.0) }
    pub fn strings(&self, key: &str) -> Vec<String> { self.get_as(key).unwrap_or_default() }

    pub fn set<T: Serialize>(&mut self, key: &str, value: T) {
        let v = serde_json::to_value(value).unwrap_or(Value::Null);
        if v.is_null() {
            self.data.remove(key);
        } else {
            self.data.insert(key.to_string(), v);
        }
        let _ = write_json(&self.path, &self.data, false);
    }
}
