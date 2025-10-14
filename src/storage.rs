use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use zip::ZipArchive;

use crate::ifdb::{Game, GameDetails};

/// Manages local storage of downloaded games, metadata, and save files
pub struct GameStorage {
    base_dir: PathBuf,
    games_dir: PathBuf,
    saves_dir: PathBuf,
    metadata_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalGame {
    pub tuid: String,
    pub title: String,
    pub author: String,
    pub file_path: PathBuf,
    pub download_date: SystemTime,
    pub file_size: u64,
    pub format: Option<String>,
    pub play_count: u32,
    pub last_played: Option<SystemTime>,
    pub description: Option<String>,
    pub cover_art_path: Option<PathBuf>,
}

impl fmt::Display for LocalGame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} by {}", self.title, self.author)?;
        if let Some(format) = &self.format {
            write!(f, " ({})", format)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveFile {
    pub game_tuid: String,
    pub save_name: String,
    pub file_path: PathBuf,
    pub save_date: SystemTime,
    pub file_size: u64,
    pub description: Option<String>,
}

impl fmt::Display for SaveFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.save_name)?;
        if let Some(desc) = &self.description {
            write!(f, " - {}", desc)?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageMetadata {
    pub version: u32,
    pub games: HashMap<String, LocalGame>,
    pub saves: HashMap<String, Vec<SaveFile>>, // Key is game TUID
}

impl GameStorage {
    pub fn new() -> Result<Self> {
        let base_dir = dirs::home_dir()
            .ok_or_else(|| anyhow!("Could not determine home directory"))?
            .join(".glkcli");

        let games_dir = base_dir.join("games");
        let saves_dir = base_dir.join("saves");
        let metadata_file = base_dir.join("metadata.json");

        // Create directories if they don't exist
        fs::create_dir_all(&base_dir).context("Failed to create base directory")?;
        fs::create_dir_all(&games_dir).context("Failed to create games directory")?;
        fs::create_dir_all(&saves_dir).context("Failed to create saves directory")?;

        Ok(GameStorage {
            base_dir,
            games_dir,
            saves_dir,
            metadata_file,
        })
    }

    /// Load metadata from disk
    pub fn load_metadata(&self) -> Result<StorageMetadata> {
        if !self.metadata_file.exists() {
            return Ok(StorageMetadata {
                version: 1,
                games: HashMap::new(),
                saves: HashMap::new(),
            });
        }

        let content = fs::read_to_string(&self.metadata_file)
            .context("Failed to read metadata file")?;

        let metadata: StorageMetadata = serde_json::from_str(&content)
            .context("Failed to parse metadata file")?;

        Ok(metadata)
    }

    /// Save metadata to disk
    pub fn save_metadata(&self, metadata: &StorageMetadata) -> Result<()> {
        let content = serde_json::to_string_pretty(metadata)
            .context("Failed to serialize metadata")?;

        fs::write(&self.metadata_file, content)
            .context("Failed to write metadata file")?;

        Ok(())
    }

    /// Get all downloaded games
    pub fn get_downloaded_games(&self) -> Result<Vec<LocalGame>> {
        let metadata = self.load_metadata()?;
        let mut games: Vec<LocalGame> = metadata.games.into_values().collect();
        
        // Sort by last played (most recent first), then by title
        games.sort_by(|a, b| {
            match (&b.last_played, &a.last_played) {
                (Some(b_time), Some(a_time)) => b_time.cmp(a_time),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.title.cmp(&b.title),
            }
        });
        
        Ok(games)
    }

    /// Check if a game is already downloaded
    pub fn is_game_downloaded(&self, tuid: &str) -> Result<bool> {
        let metadata = self.load_metadata()?;
        Ok(metadata.games.contains_key(tuid))
    }

    /// Get a specific downloaded game
    #[allow(dead_code)]
    pub fn get_game(&self, tuid: &str) -> Result<Option<LocalGame>> {
        let metadata = self.load_metadata()?;
        Ok(metadata.games.get(tuid).cloned())
    }

    /// Generate a safe filename from a game title
    fn sanitize_filename(&self, title: &str) -> String {
        title
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => c,
                ' ' => '_',
                _ => '_',
            })
            .collect::<String>()
            .trim_matches('_')
            .to_string()
    }

    /// Detect IF game files in a directory by known extensions
    fn find_if_file_in_dir(&self, dir: &Path) -> Result<Option<PathBuf>> {
        // Known IF file extensions (in priority order)
        let if_extensions = [
            "z3", "z4", "z5", "z8",  // Z-Machine
            "zblorb", "zlb",          // Z-Machine Blorb
            "ulx", "blb",             // Glulx
            "gblorb", "gbl",          // Glulx Blorb
            "gam",                    // TADS 2
            "t3",                     // TADS 3
            "acd",                    // Alan
            "a3c",                    // Alan 3
            "taf",                    // ADRIFT
            "hex",                    // Hugo
            "dat",                    // AdvSys/AGT
        ];

        // Search for files with IF extensions
        for entry in fs::read_dir(dir).context("Failed to read directory")? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    if if_extensions.contains(&ext_lower.as_str()) {
                        return Ok(Some(path));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Detect if file is a ZIP archive
    fn is_zip_file(data: &[u8]) -> bool {
        // ZIP files start with PK (0x50 0x4B)
        data.len() >= 2 && data[0] == 0x50 && data[1] == 0x4B
    }

    /// Add a downloaded game to storage
    #[allow(dead_code)]
    pub fn add_game(
        &self,
        game: &Game,
        game_details: Option<&GameDetails>,
        file_data: &[u8],
        file_extension: &str,
    ) -> Result<LocalGame> {
        let mut metadata = self.load_metadata()?;

        // Create safe filename
        let safe_title = self.sanitize_filename(&game.title);
        let filename = format!("{}_{}.{}", safe_title, &game.tuid[..8], file_extension);
        let file_path = self.games_dir.join(&filename);

        // Write game file
        fs::write(&file_path, file_data)
            .context("Failed to write game file")?;

        // Download cover art if available
        let cover_art_path = if let Some(_cover_url) = &game.cover_art_link {
            // Note: This would need to be called from an async context
            // For now, we'll skip cover art downloading in the sync version
            None
        } else if game.has_cover_art.unwrap_or(false) {
            // Check if game has cover art but no direct link
            None
        } else {
            None
        };

        let local_game = LocalGame {
            tuid: game.tuid.clone(),
            title: game.title.clone(),
            author: game.author.clone(),
            file_path,
            download_date: SystemTime::now(),
            file_size: file_data.len() as u64,
            format: game_details
                .and_then(|d| d.identification.as_ref())
                .and_then(|i| i.format.clone()),
            play_count: 0,
            last_played: None,
            description: game_details
                .and_then(|d| d.bibliographic.as_ref())
                .and_then(|b| b.description.clone()),
            cover_art_path,
        };

        // Add to metadata
        metadata.games.insert(game.tuid.clone(), local_game.clone());
        self.save_metadata(&metadata)?;

        Ok(local_game)
    }

    /// Add a downloaded game to storage with cover art (async version)
    pub async fn add_game_with_cover(
        &self,
        game: &Game,
        game_details: Option<&GameDetails>,
        file_data: &[u8],
        file_extension: &str,
    ) -> Result<LocalGame> {
        let mut metadata = self.load_metadata()?;

        // Create game-specific directory
        let safe_title = self.sanitize_filename(&game.title);
        let game_dir_name = format!("{}_{}", safe_title, &game.tuid[..8]);
        let game_dir = self.games_dir.join(&game_dir_name);
        
        fs::create_dir_all(&game_dir)
            .context("Failed to create game directory")?;

        let if_file_path: PathBuf;
        let actual_file_size = file_data.len() as u64;

        // Check if this is a ZIP file
        if Self::is_zip_file(file_data) || file_extension.to_lowercase() == "zip" {
            // Extract ZIP to game directory
            let cursor = std::io::Cursor::new(file_data);
            let mut archive = ZipArchive::new(cursor)
                .context("Failed to read ZIP archive")?;

            // Extract all files
            for i in 0..archive.len() {
                let mut file = archive.by_index(i)
                    .context("Failed to read file from ZIP")?;
                
                let outpath = game_dir.join(file.name());
                
                if file.is_dir() {
                    fs::create_dir_all(&outpath)
                        .context("Failed to create directory from ZIP")?;
                } else {
                    if let Some(parent) = outpath.parent() {
                        fs::create_dir_all(parent)
                            .context("Failed to create parent directory")?;
                    }
                    
                    let mut outfile = File::create(&outpath)
                        .with_context(|| format!("Failed to create file: {}", outpath.display()))?;
                    
                    std::io::copy(&mut file, &mut outfile)
                        .context("Failed to extract file from ZIP")?;
                }
            }

            // Find the IF file in the extracted directory
            if_file_path = self.find_if_file_in_dir(&game_dir)?
                .ok_or_else(|| anyhow!("No interactive fiction file found in ZIP archive"))?;
        } else {
            // Not a ZIP - save file directly to game directory
            let filename = format!("{}.{}", safe_title, file_extension);
            if_file_path = game_dir.join(&filename);
            
            fs::write(&if_file_path, file_data)
                .context("Failed to write game file")?;
        }

        // Download cover art if available
        let cover_art_path = if let Some(cover_url) = &game.cover_art_link {
            match self.download_cover_art(&game.tuid, cover_url).await {
                Ok(path) => Some(path),
                Err(e) => {
                    eprintln!("Warning: Failed to download cover art: {}", e);
                    None
                }
            }
        } else if game.has_cover_art.unwrap_or(false) {
            // Game has cover art but no direct link in search results
            // Try to construct cover art URL
            let cover_url = format!("https://ifdb.org/coverart?id={}", game.tuid);
            match self.download_cover_art(&game.tuid, &cover_url).await {
                Ok(path) => Some(path),
                Err(e) => {
                    eprintln!("Warning: Failed to download cover art: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let local_game = LocalGame {
            tuid: game.tuid.clone(),
            title: game.title.clone(),
            author: game.author.clone(),
            file_path: if_file_path,
            download_date: SystemTime::now(),
            file_size: actual_file_size,
            format: game_details
                .and_then(|d| d.identification.as_ref())
                .and_then(|i| i.format.clone()),
            play_count: 0,
            last_played: None,
            description: game_details
                .and_then(|d| d.bibliographic.as_ref())
                .and_then(|b| b.description.clone()),
            cover_art_path,
        };

        // Add to metadata
        metadata.games.insert(game.tuid.clone(), local_game.clone());
        self.save_metadata(&metadata)?;

        Ok(local_game)
    }

    /// Download and save cover art
    async fn download_cover_art(&self, tuid: &str, cover_url: &str) -> Result<PathBuf> {
        let client = reqwest::Client::builder()
            .user_agent("glkcli/0.1.0 IF Browser")
            .build()
            .context("Failed to create HTTP client")?;
            
        let response = client
            .get(cover_url)
            .send()
            .await
            .context("Failed to download cover art")?;

        if !response.status().is_success() {
            return Err(anyhow!("Cover art download failed: {}", response.status()));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|ct| ct.to_str().ok())
            .unwrap_or("image/jpeg");

        let extension = match content_type {
            "image/png" => "png",
            "image/gif" => "gif",
            _ => "jpg",
        };

        let filename = format!("{}_cover.{}", tuid, extension);
        let cover_path = self.base_dir.join("covers").join(&filename);

        // Create covers directory if it doesn't exist
        if let Some(parent) = cover_path.parent() {
            fs::create_dir_all(parent).context("Failed to create covers directory")?;
        }

        let bytes = response.bytes().await.context("Failed to read cover art")?;
        fs::write(&cover_path, bytes).context("Failed to write cover art")?;

        Ok(cover_path)
    }

    /// Remove a game from storage
    #[allow(dead_code)]
    pub fn remove_game(&self, tuid: &str) -> Result<()> {
        let mut metadata = self.load_metadata()?;

        if let Some(game) = metadata.games.remove(tuid) {
            // Remove game file
            if game.file_path.exists() {
                fs::remove_file(&game.file_path)
                    .context("Failed to remove game file")?;
            }

            // Remove cover art if it exists
            if let Some(cover_path) = &game.cover_art_path {
                if cover_path.exists() {
                    fs::remove_file(cover_path)
                        .context("Failed to remove cover art")?;
                }
            }

            // Remove associated save files
            if let Some(saves) = metadata.saves.remove(tuid) {
                for save in saves {
                    if save.file_path.exists() {
                        fs::remove_file(&save.file_path)
                            .context("Failed to remove save file")?;
                    }
                }
            }

            self.save_metadata(&metadata)?;
        }

        Ok(())
    }

    /// Record that a game was played
    pub fn record_game_played(&self, tuid: &str) -> Result<()> {
        let mut metadata = self.load_metadata()?;

        if let Some(game) = metadata.games.get_mut(tuid) {
            game.play_count += 1;
            game.last_played = Some(SystemTime::now());
            self.save_metadata(&metadata)?;
        }

        Ok(())
    }

    /// Get all save files for a game
    #[allow(dead_code)]
    pub fn get_save_files(&self, tuid: &str) -> Result<Vec<SaveFile>> {
        let metadata = self.load_metadata()?;
        Ok(metadata.saves.get(tuid).cloned().unwrap_or_default())
    }

    /// Get save directory for a specific game
    pub fn get_save_dir(&self, tuid: &str) -> PathBuf {
        self.saves_dir.join(tuid)
    }

    /// Discover save files for a game (scans the save directory)
    pub fn discover_save_files(&self, tuid: &str) -> Result<Vec<SaveFile>> {
        let save_dir = self.get_save_dir(tuid);
        
        if !save_dir.exists() {
            return Ok(Vec::new());
        }

        let mut saves = Vec::new();

        for entry in fs::read_dir(&save_dir).context("Failed to read save directory")? {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();

            if path.is_file() {
                let metadata = fs::metadata(&path)
                    .context("Failed to read file metadata")?;

                let save = SaveFile {
                    game_tuid: tuid.to_string(),
                    save_name: path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unnamed")
                        .to_string(),
                    file_path: path,
                    save_date: metadata
                        .modified()
                        .unwrap_or(SystemTime::UNIX_EPOCH),
                    file_size: metadata.len(),
                    description: None,
                };

                saves.push(save);
            }
        }

        // Sort by save date (newest first)
        saves.sort_by(|a, b| b.save_date.cmp(&a.save_date));

        Ok(saves)
    }

    /// Get storage statistics
    #[allow(dead_code)]
    pub fn get_stats(&self) -> Result<StorageStats> {
        let metadata = self.load_metadata()?;
        
        let total_games = metadata.games.len();
        let total_size: u64 = metadata.games.values().map(|g| g.file_size).sum();
        let total_saves: usize = metadata.saves.values().map(|saves| saves.len()).sum();

        Ok(StorageStats {
            total_games,
            total_size,
            total_saves,
        })
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct StorageStats {
    pub total_games: usize,
    pub total_size: u64,
    pub total_saves: usize,
}

impl StorageStats {
    #[allow(dead_code)]
    pub fn format_size(&self) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
        let mut size = self.total_size as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        let storage = GameStorage::new().unwrap();
        
        // Colon and space each become underscore, so "Zork: The" -> "Zork__The"
        assert_eq!(
            storage.sanitize_filename("Zork: The Great Underground Adventure!"),
            "Zork__The_Great_Underground_Adventure"
        );
        
        assert_eq!(
            storage.sanitize_filename("A/B\\C:D*E?F\"G<H>I|J"),
            "A_B_C_D_E_F_G_H_I_J"
        );
    }
}