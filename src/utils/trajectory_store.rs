use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use crate::models::problem::SWEBenchProblem;
use crate::models::ranking::ProblemContext;
use crate::models::relevance::RelevanceDecision;

/// Store for trajectory data
pub struct TrajectoryStore {
    /// Base directory for trajectory data
    base_dir: PathBuf,

    /// Problem ID
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
}
