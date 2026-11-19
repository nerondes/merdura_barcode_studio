use crate::models::{BarcodeRecord, Group};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_GROUP_NAME: &str = "Genel Koleksiyon";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoreData {
    next_group_id: i64,
    next_record_id: i64,
    groups: Vec<Group>,
    records: Vec<BarcodeRecord>,
}

impl Default for StoreData {
    fn default() -> Self {
        Self {
            next_group_id: 1,
            next_record_id: 1,
            groups: Vec::new(),
            records: Vec::new(),
        }
    }
}

/// %100 Saf Rust (C/SQLite bağımlılığı olmayan), atomik disk yazımlı yerel veri deposu.
pub struct Database {
    file_path: Option<PathBuf>,
    state: StoreData,
}

impl Database {
    pub fn open(db_path: &Path) -> Result<Self> {
        let state = if db_path.exists() {
            let raw = fs::read_to_string(db_path)
                .with_context(|| format!("Veri dosyası okunamadı: {:?}", db_path))?;
            serde_json::from_str::<StoreData>(&raw)
                .with_context(|| format!("Veri dosyası JSON formatı geçersiz: {:?}", db_path))?
        } else {
            StoreData::default()
        };

        let mut db = Self {
            file_path: Some(db_path.to_path_buf()),
            state,
        };
        db.ensure_default_group()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let mut db = Self {
            file_path: None,
            state: StoreData::default(),
        };
        db.ensure_default_group()?;
        Ok(db)
    }

    fn ensure_default_group(&mut self) -> Result<()> {
        // next_id sayaçlarını mevcut kayıtlarla her zaman tutarlı tut
        if let Some(max_gid) = self.state.groups.iter().map(|g| g.id).max()
            && self.state.next_group_id <= max_gid
        {
            self.state.next_group_id = max_gid + 1;
        }
        if let Some(max_rid) = self.state.records.iter().filter_map(|r| r.id).max()
            && self.state.next_record_id <= max_rid
        {
            self.state.next_record_id = max_rid + 1;
        }

        if self.state.groups.is_empty() {
            self.create_group(DEFAULT_GROUP_NAME)?;
        }
        Ok(())
    }

    /// Güç kesintisinde bile bozulmayı önleyen atomik yazma (.tmp -> rename).
    fn flush(&self) -> Result<()> {
        let Some(path) = &self.file_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension("json.tmp");
        let serialized = serde_json::to_string_pretty(&self.state)?;
        fs::write(&tmp_path, serialized)
            .with_context(|| format!("Geçici veri dosyası yazılamadı: {:?}", tmp_path))?;
        fs::rename(&tmp_path, path)
            .with_context(|| format!("Veri dosyası güncellenemedi: {:?}", path))?;
        Ok(())
    }

    fn current_timestamp() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{secs}")
    }

    pub fn list_groups(&self) -> Result<Vec<Group>> {
        Ok(self.state.groups.clone())
    }

    pub fn create_group(&mut self, name: &str) -> Result<i64> {
        let trimmed = name.trim();
        anyhow::ensure!(!trimmed.is_empty(), "Grup adı boş olamaz.");
        anyhow::ensure!(
            !self
                .state
                .groups
                .iter()
                .any(|g| g.name.eq_ignore_ascii_case(trimmed) || g.name == trimmed),
            "Bu isimde bir grup zaten mevcut."
        );

        let id = self.state.next_group_id;
        self.state.next_group_id += 1;
        self.state.groups.push(Group {
            id,
            name: trimmed.to_string(),
            created_at: Self::current_timestamp(),
        });
        self.flush()?;
        Ok(id)
    }

    pub fn delete_group(&mut self, group_id: i64) -> Result<()> {
        anyhow::ensure!(self.state.groups.len() > 1, "Son kalan grup silinemez.");
        self.state.groups.retain(|g| g.id != group_id);
        // ON DELETE CASCADE karşılığı: gruba ait tüm kayıtları sil
        self.state.records.retain(|r| r.group_id != group_id);
        self.flush()?;
        Ok(())
    }

    pub fn list_records(&self, group_id: i64) -> Result<Vec<BarcodeRecord>> {
        Ok(self
            .state
            .records
            .iter()
            .filter(|r| r.group_id == group_id)
            .cloned()
            .collect())
    }

    pub fn save_record(&mut self, rec: &BarcodeRecord) -> Result<i64> {
        let barcode = rec.barcode.trim();
        anyhow::ensure!(!barcode.is_empty(), "Barkod numarası boş olamaz.");
        let count = rec.print_count.max(1);

        if let Some(existing_id) = rec.id
            && let Some(slot) = self
                .state
                .records
                .iter_mut()
                .find(|r| r.id == Some(existing_id))
        {
            slot.barcode = barcode.to_string();
            slot.desc1 = rec.desc1.trim().to_string();
            slot.desc2 = rec.desc2.trim().to_string();
            slot.desc3 = rec.desc3.trim().to_string();
            slot.desc4 = rec.desc4.trim().to_string();
            slot.print_count = count;
            self.flush()?;
            return Ok(existing_id);
        }

        let new_id = self.state.next_record_id;
        self.state.next_record_id += 1;
        self.state.records.push(BarcodeRecord {
            id: Some(new_id),
            group_id: rec.group_id,
            barcode: barcode.to_string(),
            desc1: rec.desc1.trim().to_string(),
            desc2: rec.desc2.trim().to_string(),
            desc3: rec.desc3.trim().to_string(),
            desc4: rec.desc4.trim().to_string(),
            print_count: count,
        });
        self.flush()?;
        Ok(new_id)
    }

    pub fn delete_record(&mut self, record_id: i64) -> Result<()> {
        self.state.records.retain(|r| r.id != Some(record_id));
        self.flush()?;
        Ok(())
    }

    pub fn clear_group_records(&mut self, group_id: i64) -> Result<()> {
        self.state.records.retain(|r| r.group_id != group_id);
        self.flush()?;
        Ok(())
    }

    pub fn bulk_insert_records(
        &mut self,
        group_id: i64,
        records: &[BarcodeRecord],
    ) -> Result<usize> {
        let mut inserted = 0usize;
        for rec in records {
            let barcode = rec.barcode.trim();
            if barcode.is_empty() {
                continue;
            }
            let new_id = self.state.next_record_id;
            self.state.next_record_id += 1;
            self.state.records.push(BarcodeRecord {
                id: Some(new_id),
                group_id,
                barcode: barcode.to_string(),
                desc1: rec.desc1.trim().to_string(),
                desc2: rec.desc2.trim().to_string(),
                desc3: rec.desc3.trim().to_string(),
                desc4: rec.desc4.trim().to_string(),
                print_count: rec.print_count.max(1),
            });
            inserted += 1;
        }
        self.flush()?;
        Ok(inserted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_default_group_and_cascade_delete() {
        let mut db = Database::open_in_memory().unwrap();
        let groups = db.list_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, DEFAULT_GROUP_NAME);

        let g2_id = db.create_group("Sevkiyat").unwrap();
        let batch = vec![
            BarcodeRecord {
                id: None,
                group_id: g2_id,
                barcode: "8690001".into(),
                desc1: "Ürün 1".into(),
                desc2: "".into(),
                desc3: "".into(),
                desc4: "".into(),
                print_count: 2,
            },
            BarcodeRecord {
                id: None,
                group_id: g2_id,
                barcode: "8690002".into(),
                desc1: "Ürün 2".into(),
                desc2: "".into(),
                desc3: "".into(),
                desc4: "".into(),
                print_count: 1,
            },
        ];
        let count = db.bulk_insert_records(g2_id, &batch).unwrap();
        assert_eq!(count, 2);
        assert_eq!(db.list_records(g2_id).unwrap().len(), 2);

        db.delete_group(g2_id).unwrap();
        assert_eq!(db.list_records(g2_id).unwrap().len(), 0);
    }
}
