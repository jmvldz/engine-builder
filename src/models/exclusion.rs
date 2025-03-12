use serde::{Deserialize, Serialize};
use std::path::Path;
use std::fs;
use anyhow::{Result, Context};

/// Config structure for exclusion patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusionConfig {
    /// File extensions to skip
    pub extensions_to_skip: Vec<String>,
    
    /// Files to skip
    pub files_to_skip: Vec<String>,
    
    /// Directories to skip
    pub directories_to_skip: Vec<String>,
}

impl Default for ExclusionConfig {
    fn default() -> Self {
        Self {
            extensions_to_skip: vec![
                // Images
                ".png".to_string(),
                ".jpg".to_string(),
                ".jpeg".to_string(),
                ".gif".to_string(),
                ".bmp".to_string(),
                ".tif".to_string(),
                ".tiff".to_string(),
                ".ico".to_string(),
                ".svg".to_string(),
                ".webp".to_string(),
                ".heic".to_string(),

                // Audio
                ".mp3".to_string(),
                ".wav".to_string(),
                ".wma".to_string(),
                ".ogg".to_string(),
                ".flac".to_string(),
                ".m4a".to_string(),
                ".aac".to_string(),
                ".midi".to_string(),
                ".mid".to_string(),

                // Video
                ".mp4".to_string(),
                ".avi".to_string(),
                ".mkv".to_string(),
                ".mov".to_string(),
                ".wmv".to_string(),
                ".m4v".to_string(),
                ".3gp".to_string(),
                ".3g2".to_string(),
                ".rm".to_string(),
                ".swf".to_string(),
                ".flv".to_string(),
                ".webm".to_string(),
                ".mpg".to_string(),
                ".mpeg".to_string(),

                // Fonts
                ".otf".to_string(),
                ".ttf".to_string(),

                // Documents
                ".pdf".to_string(),
                ".doc".to_string(),
                ".docx".to_string(),
                ".xls".to_string(),
                ".xlsx".to_string(),
                ".ppt".to_string(),
                ".pptx".to_string(),
                ".rtf".to_string(),
                ".odt".to_string(),
                ".ods".to_string(),
                ".odp".to_string(),

                // Archives
                ".iso".to_string(),
                ".bin".to_string(),
                ".tar".to_string(),
                ".zip".to_string(),
                ".7z".to_string(),
                ".gz".to_string(),
                ".rar".to_string(),
                ".bz2".to_string(),
                ".xz".to_string(),

                // Minified and source maps
                ".min.js".to_string(),
                ".min.js.map".to_string(),
                ".js.map".to_string(),
                ".min.css".to_string(),
                ".min.css.map".to_string(),

                // Data and configuration
                ".tfstate".to_string(),
                ".tfstate.backup".to_string(),
                ".parquet".to_string(),
                ".pyc".to_string(),
                ".pub".to_string(),
                ".pem".to_string(),
                ".lock".to_string(),
                ".sqlite".to_string(),
                ".db".to_string(),
                ".env".to_string(),
                ".log".to_string(),

                // Compiled code
                ".class".to_string(),
                ".dll".to_string(),
                ".exe".to_string(),

                // Design files
                ".psd".to_string(),
                ".ai".to_string(),
                ".sketch".to_string(),

                // 3D and CAD
                ".stl".to_string(),
                ".obj".to_string(),
                ".dwg".to_string(),

                // Backup files
                ".bak".to_string(),
                ".old".to_string(),
                ".tmp".to_string(),
            ],
            
            files_to_skip: vec![
                "pnpm-lock.yaml".to_string(),
                "package-lock.json".to_string(),
                ".DS_Store".to_string(),
                ".gitignore".to_string(),
                "bun.lockb".to_string(),
                "npm-debug.log".to_string(),
                "yarn-error.log".to_string(),
                "Thumbs.db".to_string(),
                "Gemfile.lock".to_string(),
            ],
            
            directories_to_skip: vec![
                ".git".to_string(),
                "node_modules".to_string(),
                ".vscode".to_string(),
                ".idea".to_string(),
                "assets".to_string(),
                "dist".to_string(),
                "build".to_string(),
                "coverage".to_string(),
                "tmp".to_string(),
                "temp".to_string(),
                ".next".to_string(),
                ".nuxt".to_string(),
                ".cache".to_string(),
            ],
        }
    }
}

impl ExclusionConfig {
    /// Load exclusion config from a JSON file
    pub fn from_file(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)
            .context(format!("Failed to read exclusion config file: {}", path))?;
        
        let config: Self = serde_json::from_str(&content)
            .context(format!("Failed to parse exclusion config file: {}", path))?;
        
        Ok(config)
    }

    /// Check if a file should be excluded based on its extension
    pub fn should_exclude_by_extension(&self, path: &Path) -> bool {
        if let Some(extension) = path.extension() {
            if let Some(ext_str) = extension.to_str() {
                let full_ext = format!(".{}", ext_str);
                
                // Check for exact extension match
                if self.extensions_to_skip.contains(&full_ext) {
                    return true;
                }
                
                // Check for .min.js, .min.css, etc. pattern
                if let Some(file_name) = path.file_name() {
                    if let Some(name_str) = file_name.to_str() {
                        for pattern in &self.extensions_to_skip {
                            if pattern.contains(".min.") && name_str.ends_with(pattern) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        
        false
    }
    
    /// Check if a file should be excluded based on its filename
    pub fn should_exclude_by_filename(&self, path: &Path) -> bool {
        if let Some(file_name) = path.file_name() {
            if let Some(name_str) = file_name.to_str() {
                return self.files_to_skip.contains(&name_str.to_string());
            }
        }
        
        false
    }
    
    /// Check if a path should be excluded based on its parent directories
    pub fn should_exclude_by_directory(&self, path: &Path) -> bool {
        for ancestor in path.ancestors() {
            if let Some(dir_name) = ancestor.file_name() {
                if let Some(dir_str) = dir_name.to_str() {
                    if self.directories_to_skip.contains(&dir_str.to_string()) {
                        return true;
                    }
                }
            }
        }
        
        false
    }
    
    /// Check if a path should be excluded for any reason
    pub fn should_exclude(&self, path: &Path) -> bool {
        self.should_exclude_by_extension(path) || 
        self.should_exclude_by_filename(path) || 
        self.should_exclude_by_directory(path)
    }
}