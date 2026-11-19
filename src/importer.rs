use crate::models::BarcodeRecord;
use anyhow::{Context, Result};
use calamine::{Data, Reader, open_workbook_auto};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ImportedSheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
    pub max_cols: usize,
}

impl ImportedSheet {
    /// Sütun seçici ComboBox için okunabilir başlık üretir (ör. "A (1): Barkod").
    pub fn column_labels(&self, has_header: bool) -> Vec<String> {
        let mut labels = Vec::with_capacity(self.max_cols);
        let first_row = self.rows.first();
        for idx in 0..self.max_cols {
            let col_letter = excel_col_name(idx);
            let sample = if has_header {
                first_row
                    .and_then(|r| r.get(idx))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            } else {
                None
            };
            match sample {
                Some(header_txt) => labels.push(format!("{} ({}) - {}", col_letter, idx + 1, header_txt)),
                None => labels.push(format!("Sütun {} ({})", col_letter, idx + 1)),
            }
        }
        labels
    }
}

#[derive(Debug, Clone)]
pub struct ImportWorkbook {
    pub file_name: String,
    pub sheets: Vec<ImportedSheet>,
}

#[derive(Debug, Clone)]
pub struct ColumnMapping {
    pub skip_header: bool,
    pub barcode_col: Option<usize>,
    pub desc1_col: Option<usize>,
    pub desc2_col: Option<usize>,
    pub desc3_col: Option<usize>,
    pub desc4_col: Option<usize>,
    pub count_col: Option<usize>,
}

impl ColumnMapping {
    /// Sütun sayısına ve olası başlıklara göre akıllı varsayılan eşleştirme yapar.
    pub fn guess_for_sheet(sheet: &ImportedSheet) -> Self {
        let cols = sheet.max_cols;
        let mut mapping = Self {
            skip_header: sheet.rows.len() > 1,
            barcode_col: if cols >= 1 { Some(0) } else { None },
            desc1_col: if cols >= 2 { Some(1) } else { None },
            desc2_col: if cols >= 3 { Some(2) } else { None },
            desc3_col: if cols >= 4 { Some(3) } else { None },
            desc4_col: if cols >= 5 { Some(4) } else { None },
            count_col: if cols >= 6 { Some(5) } else { None },
        };

        // İlk satır gerçekten bir veri barkodu gibi tamamen rakamsa başlık atlamayı kapat
        if let Some(first) = sheet.rows.first()
            && let Some(c0) = first.first()
        {
            let trimmed = c0.trim();
            if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
                mapping.skip_header = false;
            }
        }

        mapping
    }
}

fn excel_col_name(mut index: usize) -> String {
    let mut name = String::new();
    loop {
        let rem = index % 26;
        name.insert(0, (b'A' + rem as u8) as char);
        if index < 26 {
            break;
        }
        index = (index / 26) - 1;
    }
    name
}

fn format_excel_cell(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            // Excel sayısal barkodları genellikle Float(8691234567890.0) olarak saklar;
            // ".0" ondalık kuyruğunu temizleyerek tam barkod metnine çeviriyoruz.
            if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e16 {
                format!("{:.0}", f)
            } else {
                format!("{}", f)
            }
        }
        Data::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Data::DateTime(dt) => format!("{dt}"),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.trim().to_string(),
        Data::Error(_) => String::new(),
    }
}

/// .xlsx, .xls, .ods veya .csv dosyasını MS Excel gerektirmeden belleğe okur.
pub fn load_spreadsheet(path: &Path) -> Result<ImportWorkbook> {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("dosya")
        .to_string();

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "csv" || ext == "txt" {
        let sheet = load_csv_file(path)?;
        return Ok(ImportWorkbook {
            file_name,
            sheets: vec![sheet],
        });
    }

    let mut workbook = open_workbook_auto(path)
        .with_context(|| format!("Excel dosyası açılamadı: {:?}", path))?;
    let sheet_names = workbook.sheet_names().to_vec();
    let mut sheets = Vec::new();

    for name in sheet_names {
        if let Ok(range) = workbook.worksheet_range(&name) {
            let mut rows = Vec::new();
            let mut max_cols = 0usize;
            for r in range.rows() {
                let row_vec: Vec<String> = r.iter().map(format_excel_cell).collect();
                if row_vec.iter().any(|s| !s.is_empty()) {
                    max_cols = max_cols.max(row_vec.len());
                    rows.push(row_vec);
                }
            }
            if !rows.is_empty() {
                sheets.push(ImportedSheet {
                    name,
                    rows,
                    max_cols,
                });
            }
        }
    }

    anyhow::ensure!(
        !sheets.is_empty(),
        "Seçilen dosyada veri içeren çalışma sayfası bulunamadı."
    );

    Ok(ImportWorkbook { file_name, sheets })
}

/// Türkçe Excel (; noktalı virgül), standart (, virgül) ve Sekme (\t) ayırıcılarını otomatik algılar.
fn detect_csv_delimiter(raw_bytes: &[u8]) -> u8 {
    let preview_end = raw_bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(raw_bytes.len())
        .min(2048);
    let first_line = &raw_bytes[..preview_end];

    let semicolons = first_line.iter().filter(|&&b| b == b';').count();
    let commas = first_line.iter().filter(|&&b| b == b',').count();
    let tabs = first_line.iter().filter(|&&b| b == b'\t').count();

    if tabs > semicolons && tabs > commas {
        b'\t'
    } else if semicolons >= commas && semicolons > 0 {
        b';'
    } else {
        b','
    }
}

fn load_csv_file(path: &Path) -> Result<ImportedSheet> {
    let raw = fs::read(path).with_context(|| format!("CSV dosyası okunamadı: {:?}", path))?;
    // UTF-8 BOM varsa atla
    let content_bytes = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&raw);
    let delimiter = detect_csv_delimiter(content_bytes);

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(content_bytes);

    let mut rows = Vec::new();
    let mut max_cols = 0usize;

    for record_res in rdr.records() {
        let record = record_res?;
        let row_vec: Vec<String> = record.iter().map(|s| s.trim().to_string()).collect();
        if row_vec.iter().any(|s| !s.is_empty()) {
            max_cols = max_cols.max(row_vec.len());
            rows.push(row_vec);
        }
    }

    anyhow::ensure!(!rows.is_empty(), "CSV dosyası boş.");

    Ok(ImportedSheet {
        name: "CSV".to_string(),
        rows,
        max_cols,
    })
}

/// Seçilen sütun eşleştirmesine göre kayıt listesini üretir.
pub fn extract_records(
    sheet: &ImportedSheet,
    mapping: &ColumnMapping,
    target_group_id: i64,
) -> Vec<BarcodeRecord> {
    let Some(b_col) = mapping.barcode_col else {
        return Vec::new();
    };

    let start_idx = usize::from(mapping.skip_header);
    let mut out = Vec::new();

    for row in sheet.rows.iter().skip(start_idx) {
        let get_col = |opt_idx: Option<usize>| -> String {
            opt_idx
                .and_then(|i| row.get(i))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };

        let barcode = get_col(Some(b_col));
        if barcode.is_empty() {
            continue;
        }

        let count_str = get_col(mapping.count_col);
        let print_count = count_str
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .map(|v| (v.round() as i64).max(1) as u32)
            .unwrap_or(1);

        out.push(BarcodeRecord {
            id: None,
            group_id: target_group_id,
            barcode,
            desc1: get_col(mapping.desc1_col),
            desc2: get_col(mapping.desc2_col),
            desc3: get_col(mapping.desc3_col),
            desc4: get_col(mapping.desc4_col),
            print_count,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_excel_float_barcode_formatting() {
        assert_eq!(
            format_excel_cell(&Data::Float(8691234567890.0)),
            "8691234567890"
        );
        assert_eq!(format_excel_cell(&Data::Float(12.5)), "12.5");
        assert_eq!(format_excel_cell(&Data::Int(42)), "42");
    }

    #[test]
    fn test_csv_delimiter_detection() {
        assert_eq!(detect_csv_delimiter(b"Barkod;Tanim1;Tanim2\n123;A;B"), b';');
        assert_eq!(detect_csv_delimiter(b"Barkod,Tanim1,Tanim2\n123,A,B"), b',');
        assert_eq!(detect_csv_delimiter(b"Barkod\tTanim1\n123\tA"), b'\t');
    }
}
