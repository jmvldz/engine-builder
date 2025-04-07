use anyhow::Result;
use engine_builder::stages::container::analyze_test_failure_fallback;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[cfg(test)]
mod test_utils {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    // Create a temporary directory structure for testing
    pub fn setup_test_env() -> Result<(TempDir, PathBuf, PathBuf)> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path().to_path_buf();
        
        // Create test problem directory
        let problem_dir = temp_path.join("test_problem");
        fs::create_dir_all(&problem_dir)?;
        
        // Create scripts directory
        let scripts_dir = problem_dir.join("scripts");
        fs::create_dir_all(&scripts_dir)?;
        
        // Create trajectories directory
        let trajectory_dir = problem_dir.join("trajectories");
        fs::create_dir_all(&trajectory_dir)?;
        
        // Create reasoning directory
        let reasoning_dir = trajectory_dir.join("reasoning");
        fs::create_dir_all(&reasoning_dir)?;
        
        Ok((temp_dir, problem_dir, scripts_dir))
    }

    // Create a test test script
    pub fn create_test_script(scripts_dir: &Path, content: &str) -> Result<PathBuf> {
        let test_script_path = scripts_dir.join("test-script.sh");
        let mut file = File::create(&test_script_path)?;
        file.write_all(content.as_bytes())?;
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&test_script_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&test_script_path, perms)?;
        }
        
        Ok(test_script_path)
    }
}

#[test]
fn test_analyze_test_failure_integration() {
    // Test with Dockerfile-specific errors
    let dockerfile_logs = vec![
        "Starting container".to_string(),
        "Error: command not found: python3".to_string(),
        "Error: no such file or directory: /usr/bin/gcc".to_string(),
        "Error: missing dependency libssl".to_string(),
    ];
    let (fix_dockerfile, fix_test_script) = analyze_test_failure_fallback(&dockerfile_logs);
    assert!(fix_dockerfile, "Should fix Dockerfile when Dockerfile errors are detected");
    assert!(!fix_test_script, "Should not fix test script when only Dockerfile errors are detected");

    // Test with test script-specific errors
    let test_script_logs = vec![
        "Starting container".to_string(),
        "Error: syntax error near unexpected token `('".to_string(),
        "Error: test.sh: line 10: unexpected end of file".to_string(),
        "Error: unbound variable: TEST_DIR".to_string(),
    ];
    let (fix_dockerfile, fix_test_script) = analyze_test_failure_fallback(&test_script_logs);
    assert!(!fix_dockerfile, "Should not fix Dockerfile when only test script errors are detected");
    assert!(fix_test_script, "Should fix test script when test script errors are detected");
}

#[test]
#[ignore] // Integration test that would require LLM calls - mark as ignored by default
fn test_overview_includes_test_script_error_reasoning() -> Result<()> {
    use test_utils::*;
    
    // This test is just a skeleton to show how we'd test that the test script error
    // reasoning is included in the overview document. Since this would require setting up
    // a full test pipeline with LLM calls, we've marked it as ignored.
    // 
    // In a real implementation, we would:
    // 1. Set up a test problem with a test script
    // 2. Mock the LLM to return a fixed response
    // 3. Generate an overview document
    // 4. Check that the overview includes the test script error reasoning
    
    Ok(())
}