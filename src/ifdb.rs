use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

/// IFDB API client for searching and retrieving game information
pub struct IfdbClient {
    client: Client,
    base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Game {
    pub tuid: String,
    pub title: String,
    pub link: String,
    pub author: String,
    #[serde(rename = "hasCoverArt")]
    pub has_cover_art: Option<bool>,
    pub devsys: Option<String>,
    pub published: Option<PublishedDate>,
    #[serde(rename = "averageRating")]
    pub average_rating: Option<f64>,
    #[serde(rename = "numRatings")]
    pub num_ratings: Option<u32>,
    #[serde(rename = "starRating")]
    pub star_rating: Option<f64>,
    #[serde(rename = "coverArtLink")]
    pub cover_art_link: Option<String>,
    #[serde(rename = "playTimeInMinutes")]
    pub play_time_in_minutes: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct PublishedDate {
    pub machine: String,
    pub printable: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub games: Option<Vec<Game>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GameDetails {
    pub identification: Option<Identification>,
    pub bibliographic: Option<Bibliographic>,
    #[serde(default)]
    pub contacts: ContactsField,
    pub ifdb: Option<IfdbData>,
}

impl GameDetails {
    /// Check if this game is marked as commercial (paid) and has no free downloads
    /// A game is only treated as commercial if:
    /// 1. It has the "commercial" tag, AND
    /// 2. It has no downloadable game files (all downloads are non-game or documentation)
    pub fn is_commercial(&self) -> bool {
        if let Some(ifdb) = &self.ifdb {
            // Check if has commercial tag
            let has_commercial_tag = if let Some(tags) = &ifdb.tags {
                tags.iter().any(|tag| tag.name.eq_ignore_ascii_case("commercial"))
            } else {
                false
            };
            
            // If not tagged as commercial, it's definitely free
            if !has_commercial_tag {
                return false;
            }
            
            // Check if there are any downloadable game files
            if let Some(downloads) = &ifdb.downloads {
                let has_downloadable_game = downloads.links.iter().any(|link| {
                    // Check if this is a game file (not documentation/hints)
                    link.is_game || 
                    // Also check for common game file formats
                    link.format.as_ref().map(|f| {
                        let format_lower = f.to_lowercase();
                        matches!(format_lower.as_str(), 
                            "zcode" | "zblorb" | "ulx" | "gblorb" | "glulx" | 
                            "tads" | "tads2" | "tads3" | "hugo" | "adrift" |
                            "quest" | "ink" | "twine" | "alan"
                        )
                    }).unwrap_or(false)
                });
                
                // If there are downloadable game files, treat as free despite commercial tag
                if has_downloadable_game {
                    return false;
                }
            }
            
            // Has commercial tag but no downloadable game files
            return true;
        }
        false
    }

    /// Get the purchase URL from contacts if available
    pub fn get_purchase_url(&self) -> Option<String> {
        match &self.contacts {
            ContactsField::Object(contact) => contact.url.clone(),
            ContactsField::Array(contacts) => {
                contacts.first().and_then(|c| c.url.clone())
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum ContactsField {
    Array(Vec<Contact>),
    Object(Contact),
}

impl Default for ContactsField {
    fn default() -> Self {
        ContactsField::Array(Vec::new())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Identification {
    pub ifids: Option<Vec<String>>,
    pub bafn: Option<u32>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Bibliographic {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub firstpublished: Option<String>,
    pub genre: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Contact {
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct IfdbData {
    pub tuid: String,
    #[serde(rename = "pageversion")]
    pub page_version: Option<u32>,
    pub link: String,
    pub coverart: Option<CoverArt>,
    #[serde(rename = "playTimeInMinutes")]
    pub play_time_in_minutes: Option<u32>,
    #[serde(rename = "primaryPlayOnlineUrl")]
    pub primary_play_online_url: Option<String>,
    pub downloads: Option<Downloads>,
    #[serde(rename = "averageRating")]
    pub average_rating: Option<f64>,
    #[serde(rename = "starRating")]
    pub star_rating: Option<f64>,
    #[serde(rename = "ratingCountAvg")]
    pub rating_count_avg: Option<u32>,
    #[serde(rename = "ratingCountTot")]
    pub rating_count_tot: Option<u32>,
    pub tags: Option<Vec<Tag>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct CoverArt {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Downloads {
    pub links: Vec<DownloadLink>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct DownloadLink {
    pub url: String,
    #[serde(rename = "playOnlineUrl", alias = "play_online_url")]
    pub play_online_url: Option<String>,
    pub title: String,
    pub desc: Option<String>,
    #[serde(rename = "isGame", default)]
    pub is_game: bool,
    pub format: Option<String>,
    pub os: Option<String>,
    pub compression: Option<String>,
    #[serde(rename = "compressedPrimary")]
    pub compressed_primary: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct Tag {
    pub name: String,
    pub tagcnt: Option<u32>,
    pub gamecnt: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub query: String,
    pub limit: Option<u32>,
    pub page: Option<u32>,
}

impl IfdbClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("glkcli/0.1.0 IF Browser")
            .build()
            .context("Failed to create HTTP client")?;

        Ok(IfdbClient {
            client,
            base_url: "https://ifdb.org".to_string(),
        })
    }

    /// Search for games on IFDB
    pub async fn search_games(&self, options: &SearchOptions) -> Result<Vec<Game>> {
        let mut url = format!("{}/search", self.base_url);
        let mut params = vec![
            ("json", "".to_string()),
            ("searchfor", options.query.clone()),
            ("searchgo", "Search Games".to_string()),
        ];

        // Add pagination if specified
        if let Some(limit) = options.limit {
            params.push(("count", limit.to_string()));
        }
        
        if let Some(page) = options.page {
            params.push(("pg", page.to_string()));
        }

        let query_string = params
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    k.to_string()
                } else {
                    format!("{}={}", k, urlencoding::encode(v))
                }
            })
            .collect::<Vec<_>>()
            .join("&");

        url = format!("{}?{}", url, query_string);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send search request")?;

        if !response.status().is_success() {
            return Err(anyhow!("Search request failed: {}", response.status()));
        }

        let search_response: SearchResponse = response
            .json()
            .await
            .context("Failed to parse search response")?;

        let games = search_response.games.unwrap_or_default();
        
        // Filter by devsys - only include GLK-compatible development systems
        // These are the systems that produce games we can run with GLK interpreters
        let filtered_games: Vec<Game> = games
            .into_iter()
            .filter(|game| {
                // If no devsys specified, include it (might be in multiple formats)
                let Some(devsys) = &game.devsys else {
                    return true;
                };
                
                let devsys_lower = devsys.to_lowercase();
                
                // Whitelist of GLK-compatible development systems
                let glk_systems = [
                    "inform", "tads", "adrift", "hugo", "alan", "quest",
                    "dialog", "zil", "axma", "choicescript", "dendry", 
                    "windrift"
                ];
                
                // Check if devsys contains any of our whitelisted systems
                glk_systems.iter().any(|sys| devsys_lower.contains(sys))
            })
            .collect();

        Ok(filtered_games)
    }

    /// Get detailed information about a specific game by TUID
    pub async fn get_game_details(&self, tuid: &str) -> Result<GameDetails> {
        let url = format!("{}/viewgame?json&id={}", self.base_url, tuid);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send game details request")?;

        if !response.status().is_success() {
            return Err(anyhow!("Game details request failed: {}", response.status()));
        }

        let game_details: GameDetails = response
            .json()
            .await
            .context("Failed to parse game details response")?;

        Ok(game_details)
    }

    /// Browse popular/recent games (using empty search with sorting)
    pub async fn browse_games(&self, sort_by: Option<&str>) -> Result<Vec<Game>> {
        // For browsing, we need to use the browse parameter instead of searchfor
        // to get proper rating-based sorting
        let mut url = format!("{}/search", self.base_url);
        let mut params = vec![
            ("browse", "".to_string()),
            ("json", "".to_string()),
        ];
        
        // Add sort parameter if specified
        if let Some(sort) = sort_by {
            let sort_value = match sort {
                "rating" => "ratu",  // Sort by rating (up)
                "new" => "new",       // Sort by newest
                _ => "ratu",
            };
            params.push(("sortby", sort_value.to_string()));
        }
        
        params.push(("count", "50".to_string()));

        let query_string = params
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    k.to_string()
                } else {
                    format!("{}={}", k, urlencoding::encode(v))
                }
            })
            .collect::<Vec<_>>()
            .join("&");

        url = format!("{}?{}", url, query_string);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send browse request")?;

        if !response.status().is_success() {
            return Err(anyhow!("Browse request failed: {}", response.status()));
        }

        let search_response: SearchResponse = response
            .json()
            .await
            .context("Failed to parse browse response")?;

        let games = search_response.games.unwrap_or_default();
        
        // Filter by devsys - only include GLK-compatible development systems
        let filtered_games: Vec<Game> = games
            .into_iter()
            .filter(|game| {
                // If no devsys specified, include it (might be in multiple formats)
                let Some(devsys) = &game.devsys else {
                    return true;
                };
                
                let devsys_lower = devsys.to_lowercase();
                
                // Whitelist of GLK-compatible development systems
                let glk_systems = [
                    "inform", "tads", "adrift", "hugo", "alan", "quest",
                    "dialog", "zil", "axma", "choicescript", "dendry", 
                    "windrift"
                ];
                
                // Check if devsys contains any of our whitelisted systems
                glk_systems.iter().any(|sys| devsys_lower.contains(sys))
            })
            .collect();

        Ok(filtered_games)
    }

    /// Download a file from a URL
    pub async fn download_file(&self, url: &str) -> Result<reqwest::Response> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to download file")?;

        if !response.status().is_success() {
            return Err(anyhow!("Download failed: {}", response.status()));
        }

        Ok(response)
    }
}

impl Default for IfdbClient {
    fn default() -> Self {
        Self::new().expect("Failed to create IFDB client")
    }
}

impl SearchOptions {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: None,
            page: None,
        }
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    #[allow(dead_code)]
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    pub fn with_glk_formats(mut self) -> Self {
        // Simple approach: just search for downloadable games
        // We'll filter by devsys on the client side
        let filters = "downloadable:yes";
        
        if self.query.is_empty() {
            self.query = filters.to_string();
        } else {
            self.query = format!("{} {}", self.query, filters);
        }
        self
    }
}

// Helper function for URL encoding (simple implementation)
mod urlencoding {
    pub fn encode(input: &str) -> String {
        input
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                ' ' => "+".to_string(),
                _ => format!("%{:02X}", c as u8),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_games() {
        let client = IfdbClient::new().unwrap();
        let options = SearchOptions::new("Zork").with_limit(5);
        
        // Note: This test requires network access and will fail if IFDB is down
        let result = client.search_games(&options).await;
        match result {
            Ok(games) => {
                println!("Found {} games", games.len());
                assert!(!games.is_empty());
            }
            Err(e) => {
                println!("Search failed (this is expected in CI): {}", e);
            }
        }
    }
}