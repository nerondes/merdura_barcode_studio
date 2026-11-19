use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const EMBEDDED_TR: &str = include_str!("../assets/locales/tr.json");
const EMBEDDED_EN: &str = include_str!("../assets/locales/en.json");

#[derive(Debug, Clone)]
pub struct LocaleEntry {
    pub code: String,
    pub display_name: String,
    pub strings: BTreeMap<String, String>,
}

/// Çok dilli JSON dil yöneticisi (Gömülü TR/EN + dinamik ./locales/*.json desteği).
#[derive(Debug, Clone)]
pub struct I18n {
    locales_dir: PathBuf,
    locales: BTreeMap<String, LocaleEntry>,
    active_lang: String,
}

impl I18n {
    pub fn init(locales_dir: &Path) -> Self {
        let mut manager = Self {
            locales_dir: locales_dir.to_path_buf(),
            locales: BTreeMap::new(),
            active_lang: "tr".to_string(),
        };

        // Önce gömülü TR ve EN dillerini belleğe yükle (klasör yazılamasa bile %100 çalışır)
        manager.insert_from_json_str("tr", EMBEDDED_TR);
        manager.insert_from_json_str("en", EMBEDDED_EN);

        // ./locales klasörü yoksa oluştur ve gömülü dosyaları diske çıkar
        if fs::create_dir_all(locales_dir).is_ok() {
            let tr_path = locales_dir.join("tr.json");
            if !tr_path.exists() {
                let _ = fs::write(&tr_path, EMBEDDED_TR);
            }
            let en_path = locales_dir.join("en.json");
            if !en_path.exists() {
                let _ = fs::write(&en_path, EMBEDDED_EN);
            }
        }

        // Klasördeki tüm *.json dil dosyalarını tara
        manager.reload_external_locales();
        manager
    }

    fn insert_from_json_str(&mut self, code: &str, raw_json: &str) {
        if let Ok(strings) = serde_json::from_str::<BTreeMap<String, String>>(raw_json) {
            let display_name = strings
                .get("lang_name")
                .cloned()
                .or_else(|| self.locales.get(code).map(|e| e.display_name.clone()))
                .unwrap_or_else(|| code.to_uppercase());

            if let Some(existing) = self.locales.get_mut(code) {
                existing.display_name = display_name;
                existing.strings.extend(strings);
            } else {
                self.locales.insert(
                    code.to_string(),
                    LocaleEntry {
                        code: code.to_string(),
                        display_name,
                        strings,
                    },
                );
            }
        }
    }

    /// Çalışma dizinindeki ./locales/*.json dosyalarını yeniden tarar.
    pub fn reload_external_locales(&mut self) {
        if let Ok(entries) = fs::read_dir(&self.locales_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                    && let Ok(content) = fs::read_to_string(&path)
                {
                    self.insert_from_json_str(stem, &content);
                }
            }
        }
    }

    pub fn active_lang(&self) -> &str {
        &self.active_lang
    }

    pub fn set_active_lang(&mut self, code: &str) {
        if self.locales.contains_key(code) {
            self.active_lang = code.to_string();
        }
    }

    /// Mevcut dillerin (kod, görünen ad) listesini döner.
    pub fn available_languages(&self) -> Vec<(String, String)> {
        let mut list: Vec<(String, String)> = self
            .locales
            .values()
            .map(|entry| (entry.code.clone(), format!("{} ({})", entry.display_name, entry.code.to_uppercase())))
            .collect();
        // Türkçe'yi her zaman ilk sırada tut
        list.sort_by(|a, b| {
            if a.0 == "tr" {
                std::cmp::Ordering::Less
            } else if b.0 == "tr" {
                std::cmp::Ordering::Greater
            } else {
                a.0.cmp(&b.0)
            }
        });
        list
    }

    /// Anahtar karşılığı metni döner; aktif dilde yoksa Türkçe'ye, orada da yoksa anahtarın kendisine düşer.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(entry) = self.locales.get(&self.active_lang)
            && let Some(val) = entry.strings.get(key)
        {
            return val.as_str();
        }
        if let Some(tr_entry) = self.locales.get("tr")
            && let Some(val) = tr_entry.strings.get(key)
        {
            return val.as_str();
        }
        key
    }
}
