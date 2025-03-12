use serde::{Deserialize, Serialize};

/// Represents a Dockerfile configuration generated from ranked files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerfileConfig {
    /// The base image to use
    pub base_image: String,
    
    /// Working directory in the container
    pub workdir: String,
    
    /// Files that should be copied to the container
    pub files_to_copy: Vec<String>,
    
    /// Commands to run on build (RUN instructions)
    pub build_commands: Vec<String>,
    
    /// Environment variables to set
    pub env_vars: Vec<(String, String)>,
    
    /// Ports to expose
    pub expose_ports: Vec<u16>,
    
    /// Command to run when the container starts
    pub cmd: Vec<String>,
    
    /// The complete Dockerfile content
    pub dockerfile_content: String,
}

impl DockerfileConfig {
    /// Create a new, empty Dockerfile configuration
    pub fn new() -> Self {
        Self {
            base_image: String::new(),
            workdir: String::new(),
            files_to_copy: Vec::new(),
            build_commands: Vec::new(),
            env_vars: Vec::new(),
            expose_ports: Vec::new(),
            cmd: Vec::new(),
            dockerfile_content: String::new(),
        }
    }
}