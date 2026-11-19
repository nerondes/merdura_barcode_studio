use crate::models::{BarcodeRecord, LabelTemplate};
use crate::renderer::{FontEngine, render_roll_rows};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct PrintSummary {
    pub pages_sent: usize,
    pub dpi_used: u32,
    pub warning: Option<String>,
}

/// Sistemdeki kurulu tüm yerel ve ağ yazıcılarını listeler.
pub fn list_system_printers() -> Vec<String> {
    imp::list_printers()
}

/// Kayıtları seçilen şablon ve yazıcının gerçek donanım DPI değerinde (RAII güvenli) basar.
pub fn print_records(
    printer_name: &str,
    records: &[BarcodeRecord],
    template: &LabelTemplate,
    font_engine: &FontEngine,
) -> Result<PrintSummary> {
    imp::print_records_impl(printer_name, records, template, font_engine)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::renderer::RgbaImage;
    use anyhow::{Context, bail};
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDCW, DEVMODEW, DIB_RGB_COLORS,
        DM_IN_BUFFER, DM_ORIENTATION, DM_OUT_BUFFER, DMORIENT_PORTRAIT, DeleteDC, GetDeviceCaps,
        HDC, LOGPIXELSX, LOGPIXELSY, SRCCOPY, StretchDIBits,
    };
    use windows_sys::Win32::Graphics::Printing::{
        ClosePrinter, DocumentPropertiesW, EnumPrintersW, OpenPrinterW, PRINTER_ENUM_CONNECTIONS,
        PRINTER_ENUM_LOCAL, PRINTER_INFO_4W,
    };
    use windows_sys::Win32::Storage::Xps::{
        AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage,
    };

    fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe fn wide_ptr_to_string(ptr: *mut u16) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let mut len = 0usize;
        // SAFETY: Win32 null-sonlandırmalı UTF-16 dizesi okunuyor.
        unsafe {
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            Some(String::from_utf16_lossy(slice))
        }
    }

    /// Win32 Yazıcı Handle (OpenPrinterW) kaynağını otomatik kapatan RAII sarmalayıcı.
    struct PrinterHandleGuard(HANDLE);

    impl Drop for PrinterHandleGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: OpenPrinterW ile açılan geçerli yazıcı handle'ı kapatılıyor.
                unsafe {
                    ClosePrinter(self.0);
                }
            }
        }
    }

    /// Yazıcı sürücüsünün mevcut DEVMODEW ayarlarını (kağıt boyutu, koyuluk, hız, sensör dahil) okur
    /// ve yalnızca sayfa yönünü `DMORIENT_PORTRAIT` (0° düz, sürücü döndürmesi kapalı) olarak kilitler.
    /// Böylece Windows Yazdırma Tercihleri'nde "Yatay (Landscape / 90°)" seçili kalmış olsa dahi
    /// yazıcı sürücüsü etiket satırımızı 90° çevirip alt alta bozamaz.
    pub(crate) fn build_unrotated_devmode(device_w: &[u16]) -> Option<Vec<u64>> {
        let mut hprinter: HANDLE = null_mut();
        // SAFETY: Yazıcı adı null-sonlandırmalı UTF-16 dizesidir.
        let opened = unsafe { OpenPrinterW(device_w.as_ptr(), &mut hprinter, null()) };
        if opened == 0 || hprinter.is_null() {
            return None;
        }
        let _guard = PrinterHandleGuard(hprinter);

        // 1. Sürücüye özel ekstra baytlar (dmDriverExtra - ör. Seagull/Argox ayarları) dahil tam boyutu al
        // SAFETY: fMode = 0 ile gerekli DEVMODEW tampon boyutu sorgulanıyor.
        let bytes_needed = unsafe {
            DocumentPropertiesW(
                null_mut(),
                hprinter,
                device_w.as_ptr(),
                null_mut(),
                null_mut(),
                0,
            )
        };
        if bytes_needed < size_of::<DEVMODEW>() as i32 {
            return None;
        }

        let u64_len = (bytes_needed as usize).div_ceil(size_of::<u64>());
        let mut aligned_buf = vec![0u64; u64_len];
        let dm_ptr = aligned_buf.as_mut_ptr() as *mut DEVMODEW;

        // 2. Mevcut sürücü DEVMODEW verisini tampona oku
        // SAFETY: Tampon boyutu sürücünün istediği bytes_needed kadar hizalı ayrıldı.
        let res_out = unsafe {
            DocumentPropertiesW(
                null_mut(),
                hprinter,
                device_w.as_ptr(),
                dm_ptr,
                null(),
                DM_OUT_BUFFER,
            )
        };
        if res_out < 0 {
            return None;
        }

        // 3. Sürücü döndürmesini devre dışı bırakmak için DMORIENT_PORTRAIT (0° düz) zorla
        // SAFETY: dm_ptr geçerli bir DEVMODEW başlangıcıdır.
        unsafe {
            (*dm_ptr).Anonymous1.Anonymous1.dmOrientation = DMORIENT_PORTRAIT as i16;
            (*dm_ptr).dmFields |= DM_ORIENTATION;
        }

        // 4. Değişikliği sürücüye doğrulatıp özel alanlarına (Seagull private DEVMODE) işlet
        // SAFETY: Aynı tampon hem girdi hem çıktı olarak sürücü tarafından güncelleniyor.
        let res_merge = unsafe {
            DocumentPropertiesW(
                null_mut(),
                hprinter,
                device_w.as_ptr(),
                dm_ptr,
                dm_ptr as *const DEVMODEW,
                DM_IN_BUFFER | DM_OUT_BUFFER,
            )
        };
        if res_merge < 0 {
            return None;
        }

        Some(aligned_buf)
    }

    /// Win32 HDC kaynağını kapsam dışına çıkıldığında otomatik temizleyen RAII sarmalayıcı.
    struct PrinterDcGuard(HDC);

    impl Drop for PrinterDcGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: Geçerli HDC handle'ı serbest bırakılıyor.
                unsafe {
                    DeleteDC(self.0);
                }
            }
        }
    }

    /// StartDocW ile açılan yazdırma işini hata durumunda AbortDoc, başarıda EndDoc yapan RAII sarmalayıcı.
    struct PrintJobGuard {
        hdc: HDC,
        completed: bool,
    }

    impl PrintJobGuard {
        fn finish(mut self) -> Result<()> {
            // SAFETY: Aktif yazdırma işi sonlandırılıyor.
            let rc = unsafe { EndDoc(self.hdc) };
            self.completed = true;
            if rc <= 0 {
                bail!("Win32 EndDoc başarısız oldu (kod: {rc}).");
            }
            Ok(())
        }
    }

    impl Drop for PrintJobGuard {
        fn drop(&mut self) {
            if !self.completed && !self.hdc.is_null() {
                // SAFETY: Tamamlanmamış yazdırma işi iptal edilerek yazıcı kuyruğunun kilitlenmesi önleniyor.
                unsafe {
                    AbortDoc(self.hdc);
                }
            }
        }
    }

    pub fn list_printers() -> Vec<String> {
        let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        let mut bytes_needed: u32 = 0;
        let mut count_returned: u32 = 0;

        // İlk çağrıda gerekli bellek boyutunu öğren (Level 4 hızlı yerel/ağ listelemesi sağlar)
        // SAFETY: Standart iki aşamalı Win32 EnumPrintersW boyut sorgusu.
        unsafe {
            EnumPrintersW(
                flags,
                null(),
                4,
                null_mut(),
                0,
                &mut bytes_needed,
                &mut count_returned,
            );
        }

        if bytes_needed == 0 {
            return Vec::new();
        }

        // Hizalama garantisi için u64 dizisi üzerinden tampon ayır
        let u64_len = (bytes_needed as usize).div_ceil(size_of::<u64>());
        let mut aligned_buf = vec![0u64; u64_len];
        let buf_ptr = aligned_buf.as_mut_ptr() as *mut u8;
        let buf_bytes = (u64_len * size_of::<u64>()) as u32;

        // SAFETY: Yeterli boyutta hizalı tampon ile yazıcı listesi alınıyor.
        let ok = unsafe {
            EnumPrintersW(
                flags,
                null(),
                4,
                buf_ptr,
                buf_bytes,
                &mut bytes_needed,
                &mut count_returned,
            )
        };

        if ok == 0 || count_returned == 0 {
            return Vec::new();
        }

        let mut names = Vec::with_capacity(count_returned as usize);
        // SAFETY: EnumPrintersW count_returned adet PRINTER_INFO_4W yapısını tampona doldurdu.
        unsafe {
            let info_slice = std::slice::from_raw_parts(
                buf_ptr as *const PRINTER_INFO_4W,
                count_returned as usize,
            );
            for info in info_slice {
                if let Some(name) = wide_ptr_to_string(info.pPrinterName)
                    && !name.trim().is_empty()
                {
                    names.push(name);
                }
            }
        }

        names.sort();
        names.dedup();
        names
    }

    /// RGBA görüntüyü üstten-aşağı (top-down) 32-bit BGRA DIB tamponuna çevirir (her satır doğal 4-bayt hizalı).
    fn rgba_to_bgra_dib(img: &RgbaImage) -> Vec<u8> {
        let mut bgra = Vec::with_capacity((img.width() * img.height() * 4) as usize);
        for px in img.pixels() {
            bgra.push(px[2]); // Blue
            bgra.push(px[1]); // Green
            bgra.push(px[0]); // Red
            bgra.push(255);   // Reserved / Opaque
        }
        bgra
    }

    pub fn print_records_impl(
        printer_name: &str,
        records: &[BarcodeRecord],
        template: &LabelTemplate,
        font_engine: &FontEngine,
    ) -> Result<PrintSummary> {
        let trimmed_printer = printer_name.trim();
        anyhow::ensure!(!trimmed_printer.is_empty(), "Yazıcı adı seçilmedi.");
        anyhow::ensure!(!records.is_empty(), "Yazdırılacak kayıt bulunamadı.");

        let driver_w = to_wide_null("WINSPOOL");
        let device_w = to_wide_null(trimmed_printer);
        let devmode_buf = build_unrotated_devmode(&device_w);
        let devmode_ptr = devmode_buf
            .as_ref()
            .map(|b| b.as_ptr() as *const DEVMODEW)
            .unwrap_or(null());

        // SAFETY: Seçilen Windows yazıcısı için sürücü döndürmesi kapatılmış (Portrait 0°) Device Context oluşturuluyor.
        let hdc = unsafe { CreateDCW(driver_w.as_ptr(), device_w.as_ptr(), null(), devmode_ptr) };
        if hdc.is_null() {
            bail!(
                "'{}' yazıcısına bağlanılamadı (CreateDCW başarısız). Yazıcının bağlı ve erişilebilir olduğundan emin olun.",
                trimmed_printer
            );
        }
        let dc_guard = PrinterDcGuard(hdc);

        // Yazıcının gerçek donanım DPI değerini oku (ör. Argox: 203/300 DPI, Lazer: 600 DPI)
        // SAFETY: Geçerli yazıcı DC üzerinden donanım çözünürlüğü okunuyor.
        let (hw_dpi_x, hw_dpi_y) = unsafe {
            let dx = GetDeviceCaps(dc_guard.0, LOGPIXELSX as i32).clamp(72, 2400) as u32;
            let dy = GetDeviceCaps(dc_guard.0, LOGPIXELSY as i32).clamp(72, 2400) as u32;
            (dx, dy)
        };

        let render_dpi = template.dpi_override.unwrap_or(hw_dpi_x).clamp(72, 2400);
        let (pages, warning) =
            render_roll_rows(records, template, render_dpi, font_engine, false);
        anyhow::ensure!(!pages.is_empty(), "Üretilecek etiket sayfası bulunamadı.");

        let doc_name_w = to_wide_null("Barkod Stüdyo Etiket Baskısı");
        let doc_info = DOCINFOW {
            cbSize: size_of::<DOCINFOW>() as i32,
            lpszDocName: doc_name_w.as_ptr(),
            lpszOutput: null(),
            lpszDatatype: null(),
            fwType: 0,
        };

        // SAFETY: Yazdırma belgesi başlatılıyor.
        let job_id = unsafe { StartDocW(dc_guard.0, &doc_info) };
        if job_id <= 0 {
            bail!("Yazdırma işi başlatılamadı (StartDocW hata kodu: {job_id}).");
        }
        let job_guard = PrintJobGuard {
            hdc: dc_guard.0,
            completed: false,
        };

        for (page_idx, row_img) in pages.iter().enumerate() {
            // SAFETY: Her rulo satırı ayrı bir sayfa olarak başlatılıyor.
            let sp = unsafe { StartPage(dc_guard.0) };
            if sp <= 0 {
                bail!("Sayfa {} başlatılamadı (StartPage kodu: {sp}).", page_idx + 1);
            }

            let src_w = row_img.width() as i32;
            let src_h = row_img.height() as i32;

            // render_dpi == hw_dpi olduğunda 1:1 tam piksel eşleşmesi (sıfır ölçekleme bulanıklığı)
            let dst_w = ((i64::from(src_w) * i64::from(hw_dpi_x)) / i64::from(render_dpi)).max(1) as i32;
            let dst_h = ((i64::from(src_h) * i64::from(hw_dpi_y)) / i64::from(render_dpi)).max(1) as i32;

            let bgra_bytes = rgba_to_bgra_dib(row_img);

            // SAFETY: Standart 32-bit üstten-aşağı (negatif yükseklikli) BITMAPINFO sıfırlanıp dolduruluyor.
            let mut bmi: BITMAPINFO = unsafe { zeroed() };
            bmi.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: src_w,
                biHeight: -src_h, // Negatif yükseklik = Top-Down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB as u32,
                biSizeImage: bgra_bytes.len() as u32,
                biXPelsPerMeter: ((f64::from(hw_dpi_x) * 39.3701).round()) as i32,
                biYPelsPerMeter: ((f64::from(hw_dpi_y) * 39.3701).round()) as i32,
                biClrUsed: 0,
                biClrImportant: 0,
            };

            // SAFETY: Piksel tamponu GDI yazıcı DC'sine gönderiliyor.
            let scanlines = unsafe {
                StretchDIBits(
                    dc_guard.0,
                    0,
                    0,
                    dst_w,
                    dst_h,
                    0,
                    0,
                    src_w,
                    src_h,
                    bgra_bytes.as_ptr() as *const _,
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                )
            };

            if scanlines <= 0 {
                bail!(
                    "Sayfa {} yazıcıya aktarılamadı (StretchDIBits kodu: {scanlines}).",
                    page_idx + 1
                );
            }

            // SAFETY: Sayfa tamamlanıyor.
            let ep = unsafe { EndPage(dc_guard.0) };
            if ep <= 0 {
                bail!("Sayfa {} sonlandırılamadı (EndPage kodu: {ep}).", page_idx + 1);
            }
        }

        job_guard
            .finish()
            .context("Yazdırma kuyruğu tamamlanırken hata oluştu")?;

        Ok(PrintSummary {
            pages_sent: pages.len(),
            dpi_used: render_dpi,
            warning,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::Graphics::Gdi::{HORZRES, HORZSIZE, VERTRES, VERTSIZE};

        #[test]
        fn test_printer_devmode_unrotated_dimensions() {
            let printers = list_printers();
            let Some(target) = printers
                .iter()
                .find(|p| p.to_lowercase().contains("argox"))
                .or_else(|| printers.first())
            else {
                return;
            };

            let driver_w = to_wide_null("WINSPOOL");
            let device_w = to_wide_null(target);

            // 1. Sürücü varsayılanı (null DEVMODE)
            unsafe {
                let hdc_raw = CreateDCW(driver_w.as_ptr(), device_w.as_ptr(), null(), null());
                if !hdc_raw.is_null() {
                    let _g = PrinterDcGuard(hdc_raw);
                    let hs = GetDeviceCaps(hdc_raw, HORZSIZE as i32);
                    let vs = GetDeviceCaps(hdc_raw, VERTSIZE as i32);
                    let hr = GetDeviceCaps(hdc_raw, HORZRES as i32);
                    let vr = GetDeviceCaps(hdc_raw, VERTRES as i32);
                    println!("[RAW DEVMODE] {target}: {hs}x{vs} mm ({hr}x{vr} px)");
                }
            }

            // 2. Bizim DMORIENT_PORTRAIT kilitli DEVMODEW yapımız
            let dm_buf = build_unrotated_devmode(&device_w);
            assert!(dm_buf.is_some(), "DEVMODEW okunabilmeli");
            let dm_ptr = dm_buf.as_ref().unwrap().as_ptr() as *const DEVMODEW;
            unsafe {
                let hdc_fixed = CreateDCW(driver_w.as_ptr(), device_w.as_ptr(), null(), dm_ptr);
                assert!(!hdc_fixed.is_null());
                let _g = PrinterDcGuard(hdc_fixed);
                let hs = GetDeviceCaps(hdc_fixed, HORZSIZE as i32);
                let vs = GetDeviceCaps(hdc_fixed, VERTSIZE as i32);
                let hr = GetDeviceCaps(hdc_fixed, HORZRES as i32);
                let vr = GetDeviceCaps(hdc_fixed, VERTRES as i32);
                println!("[PORTRAIT DEVMODE] {target}: {hs}x{vs} mm ({hr}x{vr} px)");
                assert!(hr > 0 && vr > 0);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use anyhow::Context;
    use std::fs;
    use std::path::Path;

    pub fn list_printers() -> Vec<String> {
        vec![
            "Mock Argox Thermal (203 DPI - PNG Export)".to_string(),
            "Mock Argox Thermal (300 DPI - PNG Export)".to_string(),
            "Mock Office Laser (600 DPI - PNG Export)".to_string(),
        ]
    }

    pub fn print_records_impl(
        printer_name: &str,
        records: &[BarcodeRecord],
        template: &LabelTemplate,
        font_engine: &FontEngine,
    ) -> Result<PrintSummary> {
        let inferred_dpi = if printer_name.contains("600") {
            600
        } else if printer_name.contains("300") {
            300
        } else {
            203
        };
        let render_dpi = template.dpi_override.unwrap_or(inferred_dpi).clamp(72, 2400);
        let (pages, warning) =
            render_roll_rows(records, template, render_dpi, font_engine, false);

        let out_dir = Path::new("print_preview_out");
        fs::create_dir_all(out_dir)
            .context("Mock PNG çıktı klasörü oluşturulamadı")?;

        for (idx, img) in pages.iter().enumerate() {
            let file_path = out_dir.join(format!("etiket_satir_{:03}.bmp", idx + 1));
            img.save_bmp(&file_path)
                .with_context(|| format!("BMP kaydedilemedi: {:?}", file_path))?;
        }

        Ok(PrintSummary {
            pages_sent: pages.len(),
            dpi_used: render_dpi,
            warning,
        })
    }
}
