use crate::config::*;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub fn detect_format_by_header(file_path: &Path) -> Result<GameFormat> {
    let mut file = File::open(file_path)
        .with_context(|| format!("Failed to open file: {}", file_path.display()))?;
    
    let mut header = [0u8; 32];
    let bytes_read = file.read(&mut header)
        .context("Failed to read file header")?;
    
    if bytes_read < 4 {
        return Ok(GameFormat::Unknown);
    }
    
    // Check for Blorb format first
    if bytes_read >= 12 && &header[0..4] == b"FORM" && &header[8..12] == b"IFRS" {
        return detect_format_by_blorb(file_path);
    }
    
    // Check magic patterns
    for pattern in MAGIC_PATTERNS {
        if bytes_read >= pattern.pattern.len() && &header[..pattern.pattern.len()] == pattern.pattern {
            return Ok(pattern.format);
        }
    }
    
    // Special handling for Z-code
    if header[0] >= 1 && header[0] <= 8 && bytes_read >= 26 {
        // Additional Z-code validation could go here
        return Ok(GameFormat::ZCode);
    }
    
    // Special handling for Hugo
    if bytes_read >= 7 && header[3] == b'-' && header[6] == b'-' {
        return Ok(GameFormat::Hugo);
    }
    
    // Additional validation for Adrift TAF files
    if bytes_read >= 8 && &header[0..8] == b"Version " {
        // Look for version numbers like "3.9" or "4.0" following "Version "
        if bytes_read >= 12 {
            let version_part = std::str::from_utf8(&header[8..12]).unwrap_or("");
            if version_part.starts_with("3.9") || version_part.starts_with("4.0") {
                return Ok(GameFormat::Adrift);
            }
        }
        // Default to Adrift if we found "Version " but couldn't parse version
        return Ok(GameFormat::Adrift);
    }
    
    // Special handling for ZIP files with .adrift extension
    if bytes_read >= 4 && &header[0..4] == b"PK\x03\x04" {
        let extension = file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase());
        
        if let Some(ext) = extension {
            if ext == "adrift" {
                return Ok(GameFormat::Adrift);
            }
        }
    }
    
    Ok(GameFormat::Unknown)
}

pub fn detect_format_by_extension(file_path: &Path) -> GameFormat {
    let extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_lowercase());
    
    if let Some(ext) = extension {
        for mapping in EXTENSION_MAPPINGS {
            if ext == mapping.extension {
                return mapping.format;
            }
        }
    }
    
    GameFormat::Unknown
}

fn detect_format_by_blorb(file_path: &Path) -> Result<GameFormat> {
    let mut file = File::open(file_path)
        .with_context(|| format!("Failed to open Blorb file: {}", file_path.display()))?;
    
    // Skip FORM header and look for RIdx
    file.seek(SeekFrom::Start(12))
        .context("Failed to seek in Blorb file")?;
    
    let mut ridx = [0u8; 4];
    file.read_exact(&mut ridx)
        .context("Failed to read RIdx header")?;
    
    if &ridx != b"RIdx" {
        return Ok(GameFormat::Unknown);
    }
    
    // Read RIdx size
    let mut size_bytes = [0u8; 4];
    file.read_exact(&mut size_bytes)
        .context("Failed to read RIdx size")?;
    
    // Skip resource count (4 bytes) and read first resource entry
    file.seek(SeekFrom::Current(4))
        .context("Failed to seek past resource count")?;
    
    // Read first resource type (4 bytes)
    let mut resource_type = [0u8; 4];
    if file.read_exact(&mut resource_type).is_ok() && &resource_type == b"Exec" {
        // Skip resource number (4 bytes), read offset (4 bytes)
        file.seek(SeekFrom::Current(4))
            .context("Failed to seek past resource number")?;
        
        let mut offset_bytes = [0u8; 4];
        if file.read_exact(&mut offset_bytes).is_ok() {
            let offset = u32::from_be_bytes(offset_bytes) as u64;
            
            // Jump to the executable data and check its format
            file.seek(SeekFrom::Start(offset))
                .context("Failed to seek to executable data")?;
            
            let mut exec_header = [0u8; 4];
            if file.read_exact(&mut exec_header).is_ok() {
                if &exec_header == b"Glul" || &exec_header == b"GLUL" {
                    return Ok(GameFormat::Glulx);
                } else if exec_header[0] >= 1 && exec_header[0] <= 8 {
                    // Z-code version
                    return Ok(GameFormat::ZCode);
                }
            }
        }
    }
    
    // Fallback: assume Z-code as it's most common in Blorb files
    Ok(GameFormat::ZCode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_file(data: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        file.write_all(data).expect("Failed to write test data");
        file.flush().expect("Failed to flush file");
        file
    }

    #[test]
    fn test_detect_zcode_by_header() {
        // Z-code version 5 header (minimal valid header)
        let mut data = vec![0u8; 32];
        data[0] = 5; // Z-code version 5
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::ZCode);
    }

    #[test]
    fn test_detect_zcode_all_versions() {
        for version in 1..=8 {
            let mut data = vec![0u8; 32];
            data[0] = version;
            let file = create_test_file(&data);
            
            let format = detect_format_by_header(file.path()).unwrap();
            assert_eq!(format, GameFormat::ZCode, "Version {} should be detected", version);
        }
    }

    #[test]
    fn test_detect_glulx_by_header() {
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(b"Glul");
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Glulx);
    }

    #[test]
    fn test_detect_tads2_by_header() {
        let mut data = vec![0u8; 32];
        data[0..12].copy_from_slice(b"TADS2 bin\x0A\x0D\x1A");
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Tads);
    }

    #[test]
    fn test_detect_tads3_by_header() {
        let mut data = vec![0u8; 32];
        data[0..7].copy_from_slice(b"TADS3 r");
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Tads);
    }

    #[test]
    fn test_detect_hugo_by_header() {
        let mut data = vec![0u8; 32];
        data[3] = b'-';
        data[6] = b'-';
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Hugo);
    }

    #[test]
    fn test_detect_adrift_by_header() {
        let mut data = vec![0u8; 32];
        data[0..12].copy_from_slice(b"Version 3.9 ");
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Adrift);
    }

    #[test]
    fn test_detect_adrift5_by_header() {
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(b"\x3C\x42\x3F\xC9");
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Adrift5);
    }

    #[test]
    fn test_detect_unknown_small_file() {
        let data = vec![0u8; 2]; // Too small
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Unknown);
    }

    #[test]
    fn test_detect_unknown_random_data() {
        let data = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22];
        let file = create_test_file(&data);
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Unknown);
    }

    #[test]
    fn test_detect_format_by_extension_zcode() {
        assert_eq!(detect_format_by_extension(Path::new("game.z5")), GameFormat::ZCode);
        assert_eq!(detect_format_by_extension(Path::new("game.z8")), GameFormat::ZCode);
        assert_eq!(detect_format_by_extension(Path::new("game.dat")), GameFormat::ZCode);
        assert_eq!(detect_format_by_extension(Path::new("/path/to/game.Z5")), GameFormat::ZCode); // Case insensitive
    }

    #[test]
    fn test_detect_format_by_extension_glulx() {
        assert_eq!(detect_format_by_extension(Path::new("game.ulx")), GameFormat::Glulx);
        assert_eq!(detect_format_by_extension(Path::new("game.ULX")), GameFormat::Glulx);
    }

    #[test]
    fn test_detect_format_by_extension_tads() {
        assert_eq!(detect_format_by_extension(Path::new("game.gam")), GameFormat::Tads);
        assert_eq!(detect_format_by_extension(Path::new("game.t3")), GameFormat::Tads);
    }

    #[test]
    fn test_detect_format_by_extension_hugo() {
        assert_eq!(detect_format_by_extension(Path::new("game.hex")), GameFormat::Hugo);
    }

    #[test]
    fn test_detect_format_by_extension_adrift() {
        assert_eq!(detect_format_by_extension(Path::new("game.taf")), GameFormat::Adrift);
        assert_eq!(detect_format_by_extension(Path::new("game.adrift")), GameFormat::Adrift);
    }

    #[test]
    fn test_detect_format_by_extension_unknown() {
        assert_eq!(detect_format_by_extension(Path::new("game.txt")), GameFormat::Unknown);
        assert_eq!(detect_format_by_extension(Path::new("game.zip")), GameFormat::Unknown);
        assert_eq!(detect_format_by_extension(Path::new("game")), GameFormat::Unknown);
    }

    #[test]
    fn test_detect_format_nonexistent_file() {
        let result = detect_format_by_header(Path::new("/nonexistent/file.z5"));
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_blorb_format() {
        // Create a minimal Blorb file with Glulx executable
        let mut data = vec![0u8; 256];
        data[0..4].copy_from_slice(b"FORM");
        data[4..8].copy_from_slice(&100u32.to_be_bytes()); // Size
        data[8..12].copy_from_slice(b"IFRS");
        data[12..16].copy_from_slice(b"RIdx");
        data[16..20].copy_from_slice(&32u32.to_be_bytes()); // RIdx size
        data[20..24].copy_from_slice(&1u32.to_be_bytes()); // Resource count
        data[24..28].copy_from_slice(b"Exec");
        data[28..32].copy_from_slice(&0u32.to_be_bytes()); // Resource number
        data[32..36].copy_from_slice(&64u32.to_be_bytes()); // Offset to executable
        data[64..68].copy_from_slice(b"Glul");
        
        let file = create_test_file(&data);
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Glulx);
    }

    #[test]
    fn test_detect_zip_with_adrift_extension() {
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(b"PK\x03\x04"); // ZIP signature
        
        let mut file = NamedTempFile::with_suffix(".adrift").expect("Failed to create temp file");
        file.write_all(&data).expect("Failed to write test data");
        file.flush().expect("Failed to flush file");
        
        let format = detect_format_by_header(file.path()).unwrap();
        assert_eq!(format, GameFormat::Adrift);
    }
}
