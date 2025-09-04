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
