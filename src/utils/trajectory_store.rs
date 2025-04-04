use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use crate::models::problem::SWEBenchProblem;
use crate::models::ranking::ProblemContext;
use crate::models::relevance::RelevanceDecision;
use crate::models::overview::OverviewData;

/// Store for trajectory data
pub struct TrajectoryStore {
    /// Base directory for trajectory data
    base_dir: PathBuf,

    /// Problem ID
    #[allow(dead_code)]
    problem_id: String,
}

impl TrajectoryStore {
    /// Create a new trajectory store
    pub fn new<P: AsRef<Path>>(base_dir: P, problem: &SWEBenchProblem) -> Result<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();

        // Create the base directory if it doesn't exist
        fs::create_dir_all(&base_dir).context(format!(
            "Failed to create trajectory directory: {:?}",
            base_dir
        ))?;

        Ok(Self {
            base_dir,
            problem_id: problem.id.clone(),
        })
    }
    
    /// Get the path to the overview data file
    pub fn overview_data_path(&self) -> PathBuf {
        self.problem_dir().join("overview_data.json")
    }
    
    /// Get the path to the overview markdown file
    pub fn overview_md_path(&self) -> PathBuf {
        self.problem_dir().join("overview.md")
    }
    
    /// Get the path to the reasoning directory
    pub fn reasoning_dir(&self) -> PathBuf {
        self.problem_dir().join("reasoning")
    }
    
    /// Get the path for storing reasoning for a specific stage
    pub fn reasoning_path(&self, stage: &str, suffix: &str) -> PathBuf {
        let reasoning_dir = self.reasoning_dir();
        reasoning_dir.join(format!("{}_{}{}.json", stage, self.problem_id, suffix))
    }

    /// Get the path to the problem directory
    pub fn problem_dir(&self) -> PathBuf {
        self.base_dir.clone()
    }

    /// Get the path to the relevance decisions file
    pub fn relevance_decisions_path(&self) -> PathBuf {
        self.problem_dir().join("relevance_decisions.json")
    }
    
    /// Ensure the base directory exists
    fn ensure_base_dir_exists(&self) -> Result<()> {
        let dir = self.base_dir.clone();
        fs::create_dir_all(&dir).context(format!(
            "Failed to create base directory: {:?}",
            dir
        ))?;
        Ok(())
    }

    /// Get the path to the file ranking
    fn ranking_path(&self) -> PathBuf {
        self.problem_dir().join("ranking.json")
    }

    /// Check if a relevance decision exists for a file
    pub fn relevance_decision_exists(&self, file_path: &str) -> bool {
        let decisions = self.load_relevance_decisions().unwrap_or_default();
        decisions.contains_key(file_path)
    }

    /// Load all relevance decisions from the relevance_decisions.json file
    pub fn load_relevance_decisions(&self) -> Result<HashMap<String, RelevanceDecision>> {
        let path = self.relevance_decisions_path();

        if !path.exists() {
            log::warn!("Relevance decisions file not found at: {:?}", path);
            return Ok(HashMap::new());
        }

        let file = File::open(&path).context(format!(
            "Failed to open relevance decisions file: {:?}",
            path
        ))?;
        let reader = BufReader::new(file);

        let decisions: HashMap<String, RelevanceDecision> =
            serde_json::from_reader(reader).context("Failed to parse relevance decisions")?;

        Ok(decisions)
    }
    
    /// Load all relevance decisions from the consolidated file
    pub fn load_all_relevance_decisions(&self) -> Result<HashMap<String, RelevanceDecision>> {
        // Just use the existing load_relevance_decisions method that reads from the consolidated file
        self.load_relevance_decisions()
    }

    /// Save a relevance decision for a file
    pub fn save_per_file_relevance_decision(
        &self,
        file_path: &str,
        decision: RelevanceDecision,
    ) -> Result<()> {
        // Ensure the base directory exists
        self.ensure_base_dir_exists()?;
        
        // Save to the consolidated relevance_decisions.json file
        let path = self.relevance_decisions_path();

        // Load existing decisions
        let mut decisions = self.load_relevance_decisions().unwrap_or_default();

        // Add or update the decision for this file
        decisions.insert(file_path.to_string(), decision);

        // Save all decisions
        let file = File::create(&path).context(format!(
            "Failed to create relevance decisions file: {:?}",
            path
        ))?;
        let writer = BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &decisions)
            .context("Failed to write relevance decisions")?;

        Ok(())
    }

    /// Check if a ranking exists
    pub fn ranking_exists(&self) -> bool {
        self.ranking_path().exists()
    }

    /// Save the file ranking
    pub fn save_ranking(&self, context: ProblemContext) -> Result<()> {
        // Ensure the base directory exists
        self.ensure_base_dir_exists()?;
        
        let path = self.ranking_path();

        let file =
            File::create(&path).context(format!("Failed to create ranking file: {:?}", path))?;
        let writer = BufWriter::new(file);

        serde_json::to_writer_pretty(writer, &context).context("Failed to write ranking")?;

        Ok(())
    }

    /// Load the file ranking
    pub fn load_ranking(&self) -> Result<ProblemContext> {
        let path = self.ranking_path();

        if !path.exists() {
            return Err(anyhow::anyhow!("Ranking file does not exist"));
        }

        let file = File::open(&path).context(format!("Failed to open ranking file: {:?}", path))?;
        let reader = BufReader::new(file);

        let context: ProblemContext =
            serde_json::from_reader(reader).context("Failed to parse ranking")?;

        Ok(context)
    }
    
    /// Check if overview data exists
    pub fn overview_data_exists(&self) -> bool {
        self.overview_data_path().exists()
    }
    
    /// Save overview data
    pub fn save_overview_data(&self, overview: &OverviewData) -> Result<()> {
        // Ensure the base directory exists
        self.ensure_base_dir_exists()?;
        
        let path = self.overview_data_path();
        
        let file = File::create(&path).context(format!(
            "Failed to create overview data file: {:?}",
            path
        ))?;
        let writer = BufWriter::new(file);
        
        serde_json::to_writer_pretty(writer, overview).context("Failed to write overview data")?;
        
        // Also generate and save the markdown file
        let md_content = overview.to_markdown();
        let md_path = self.overview_md_path();
        
        fs::write(&md_path, md_content).context(format!(
            "Failed to write overview markdown to {:?}",
            md_path
        ))?;
        
        Ok(())
    }
    
    /// Load overview data
    pub fn load_overview_data(&self) -> Result<OverviewData> {
        let path = self.overview_data_path();
        
        if !path.exists() {
            return Err(anyhow::anyhow!("Overview data file does not exist"));
        }
        
        let file = File::open(&path).context(format!(
            "Failed to open overview data file: {:?}",
            path
        ))?;
        let reader = BufReader::new(file);
        
        let overview: OverviewData = serde_json::from_reader(reader)
            .context("Failed to parse overview data")?;
            
        Ok(overview)
    }
    
    /// Save reasoning for a specific stage
    pub fn save_stage_reasoning(&self, stage: &str, suffix: &str, reasoning: &str, metadata: Option<serde_json::Value>) -> Result<()> {
        // Ensure the reasoning directory exists
        let reasoning_dir = self.reasoning_dir();
        fs::create_dir_all(&reasoning_dir).context(format!(
            "Failed to create reasoning directory: {:?}",
            reasoning_dir
        ))?;
        
        let path = self.reasoning_path(stage, suffix);
        
        // Create a structure with reasoning and metadata
        let mut data = serde_json::Map::new();
        data.insert("reasoning".to_string(), serde_json::Value::String(reasoning.to_string()));
        
        // Add timestamp
        data.insert("timestamp".to_string(), serde_json::Value::String(chrono::Utc::now().to_rfc3339()));
        
        // Add stage
        data.insert("stage".to_string(), serde_json::Value::String(stage.to_string()));
        
        // Add problem_id
        data.insert("problem_id".to_string(), serde_json::Value::String(self.problem_id.clone()));
        
        // Add optional metadata
        if let Some(meta) = metadata {
            data.insert("metadata".to_string(), meta);
        }
        
        let json_value = serde_json::Value::Object(data);
        
        let file = File::create(&path).context(format!(
            "Failed to create reasoning file: {:?}",
            path
        ))?;
        let writer = BufWriter::new(file);
        
        serde_json::to_writer_pretty(writer, &json_value).context("Failed to write reasoning data")?;
        
        Ok(())
    }
    
    /// Load reasoning for a specific stage
    pub fn load_stage_reasoning(&self, stage: &str, suffix: &str) -> Result<(String, Option<serde_json::Value>)> {
        let path = self.reasoning_path(stage, suffix);
        
        if !path.exists() {
            return Err(anyhow::anyhow!("Reasoning file does not exist: {:?}", path));
        }
        
        let file = File::open(&path).context(format!(
            "Failed to open reasoning file: {:?}",
            path
        ))?;
        let reader = BufReader::new(file);
        
        let data: serde_json::Value = serde_json::from_reader(reader)
            .context("Failed to parse reasoning data")?;
            
        // Extract reasoning and metadata
        let reasoning = data.get("reasoning")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing reasoning field in {:?}", path))?;
            
        let metadata = data.get("metadata").cloned();
        
        Ok((reasoning, metadata))
    }
    
    /// List all reasoning files for a problem
    pub fn list_reasoning_files(&self) -> Result<Vec<PathBuf>> {
        let reasoning_dir = self.reasoning_dir();
        
        if !reasoning_dir.exists() {
            return Ok(Vec::new());
        }
        
        let entries = fs::read_dir(&reasoning_dir)
            .context(format!("Failed to read reasoning directory: {:?}", reasoning_dir))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().map_or(false, |ext| ext == "json"))
            .collect();
            
        Ok(entries)
    }
}
