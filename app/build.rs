//! Embeds the app icon (drawn by ups_core::icon) and version info into the exe.

use std::path::Path;
use std::{env, fs};

use ups_core::UpsState;
use ups_core::icon::tray_rgba;

/// Multi-size .ico with 32-bit DIB images.
fn write_ico(path: &Path, sizes: &[u32]) {
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&s| {
            let rgba = tray_rgba(s, UpsState::Online, true, Some(100));
            let mask_row = s.div_ceil(32) as usize * 4;
            let mut img = Vec::new();
            let header: [u32; 10] = [40, s, s * 2, 0, 0, 0, 0, 0, 0, 0];
            for (i, v) in header.iter().enumerate() {
                match i {
                    // biPlanes (u16) + biBitCount (u16) share one u32 slot.
                    3 => img.extend_from_slice(&[1, 0, 32, 0]),
                    _ => img.extend_from_slice(&v.to_le_bytes()),
                }
            }
            for y in (0..s).rev() {
                for x in 0..s {
                    let i = ((y * s + x) * 4) as usize;
                    img.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            img.extend(std::iter::repeat_n(0u8, mask_row * s as usize));
            img
        })
        .collect();

    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&s, img) in sizes.iter().zip(&images) {
        let dim = if s >= 256 { 0 } else { s as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&(img.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in &images {
        ico.extend_from_slice(img);
    }
    fs::write(path, ico).expect("write icon");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../core/src/icon.rs");

    let out = env::var("OUT_DIR").unwrap();
    let ico = Path::new(&out).join("app.ico");
    // No 256 px entry: uncompressed it would add ~260 KB to the exe.
    write_ico(&ico, &[16, 20, 24, 32, 40, 48, 64]);

    let ver = env::var("CARGO_PKG_VERSION").unwrap();
    let parts: Vec<&str> = ver.split('.').collect();
    let num = format!("{},{},{},0", parts[0], parts[1], parts[2]);
    let rc = format!(
        r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {num}
PRODUCTVERSION {num}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Small UPS Monitor"
      VALUE "ProductName", "Small UPS Monitor"
      VALUE "FileVersion", "{ver}"
      VALUE "ProductVersion", "{ver}"
      VALUE "OriginalFilename", "small-ups-monitor.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = ico.display().to_string().replace('\\', "\\\\"),
    );
    let rc_path = Path::new(&out).join("app.rc");
    fs::write(&rc_path, rc).expect("write rc");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
