use std::collections::HashMap;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub timestamp: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    pub path: String,
    pub change_type: FileChangeType,
}

#[derive(Debug, PartialEq)]
pub struct ParsedCommits {
    pub commits: Vec<Commit>,
    pub file_changes: Vec<FileChange>,
}

// Serde structs for parsing jj json(self) output
#[derive(Debug, Deserialize)]
pub struct JJCommit {
    pub commit_id: String,
    pub description: String,
    pub author: JJSignature,
    pub committer: JJSignature,
}

#[derive(Debug, Deserialize)]
pub struct JJSignature {
    pub name: String,
    pub email: String,
    pub timestamp: String,
}

impl FileChangeType {
    fn from_char(c: char) -> Option<Self> {
        match c {
            'A' => Some(FileChangeType::Added),
            'M' => Some(FileChangeType::Modified),
            'D' => Some(FileChangeType::Deleted),
            'R' => Some(FileChangeType::Renamed),
            _ => None,
        }
    }
}

impl FileChange {
    fn from_status_line(line: &str) -> Option<Self> {
        if line.len() < 3 {
            return None;
        }
        
        let change_type = FileChangeType::from_char(line.chars().next()?)?;
        let path = line[2..].to_string(); // Skip change type and space
        
        Some(FileChange { path, change_type })
    }
}

pub fn parse_jj_jsonl_output(jsonl_output: &str, diff_output: &str) -> Result<ParsedCommits, String> {
    let mut commits = Vec::new();
    
    // Parse JSONL (JSON Lines) format
    for line in jsonl_output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        
        let jj_commit: JJCommit = serde_json::from_str(line)
            .map_err(|e| format!("Failed to parse JSON line: {} - Error: {}", line, e))?;
        
        // Convert to our Commit struct
        let commit = Commit {
            hash: jj_commit.commit_id[..12.min(jj_commit.commit_id.len())].to_string(), // Use first 12 chars like short hash
            author: jj_commit.author.email,
            timestamp: jj_commit.author.timestamp[..19].replace('T', " "), // Convert ISO to readable format
            description: jj_commit.description.trim().to_string(),
        };
        
        commits.push(commit);
    }
    
    // Parse file changes from diff output
    let mut all_file_changes = Vec::new();
    for line in diff_output.lines() {
        if let Some(change) = FileChange::from_status_line(line) {
            all_file_changes.push(change);
        }
    }
    
    // Aggregate file changes to show final state
    let aggregated_changes = aggregate_file_changes(all_file_changes);
    
    Ok(ParsedCommits {
        commits,
        file_changes: aggregated_changes,
    })
}

fn aggregate_file_changes(changes: Vec<FileChange>) -> Vec<FileChange> {
    let mut file_states: HashMap<String, FileChangeType> = HashMap::new();
    
    // Process changes in order, keeping track of final state for each file
    for change in changes {
        match change.change_type {
            FileChangeType::Added => {
                file_states.insert(change.path, FileChangeType::Added);
            }
            FileChangeType::Modified => {
                // If file was previously added, keep it as added
                // Otherwise, mark as modified
                if !file_states.contains_key(&change.path) {
                    file_states.insert(change.path, FileChangeType::Modified);
                }
            }
            FileChangeType::Deleted => {
                // If file was added in this changeset, remove it entirely
                // Otherwise, mark as deleted
                match file_states.get(&change.path) {
                    Some(FileChangeType::Added) => {
                        file_states.remove(&change.path);
                    }
                    _ => {
                        file_states.insert(change.path, FileChangeType::Deleted);
                    }
                }
            }
            FileChangeType::Renamed => {
                // For simplicity, treat rename as modified
                file_states.insert(change.path, FileChangeType::Modified);
            }
        }
    }
    
    // Convert back to Vec<FileChange>
    let mut result: Vec<FileChange> = file_states
        .into_iter()
        .map(|(path, change_type)| FileChange { path, change_type })
        .collect();
    
    // Sort by path for consistent output
    result.sort_by(|a, b| a.path.cmp(&b.path));
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jsonl_single_commit() {
        let jsonl_output = r#"{"commit_id":"7dee30d4d055f7b38e7a0328271e7cb51a76a9b1","parents":["9c07519a4dae8f65aa19a5a5e8a3625371980d65"],"change_id":"wmqrlkqzpqqxvqqukqnluvwlknowwmyw","description":"feat(ping-discord): Add revision parameter","author":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T01:15:47+01:00"},"committer":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T01:21:05+01:00"}}"#;
        
        let diff_output = "M tools/ping-discord/Cargo.toml\nA tools/ping-discord/src/main.rs";
        
        let result = parse_jj_jsonl_output(jsonl_output, diff_output).unwrap();
        
        assert_eq!(result.commits.len(), 1);
        assert_eq!(result.commits[0].hash, "7dee30d4d055");
        assert_eq!(result.commits[0].author, "sveina@gmail.com");
        assert_eq!(result.commits[0].description, "feat(ping-discord): Add revision parameter");
        assert_eq!(result.commits[0].timestamp, "2025-07-04 01:15:47");
        
        // Files should be sorted by path
        assert_eq!(result.file_changes.len(), 2);
        assert_eq!(result.file_changes[0].path, "tools/ping-discord/Cargo.toml");
        assert_eq!(result.file_changes[0].change_type, FileChangeType::Modified);
        assert_eq!(result.file_changes[1].path, "tools/ping-discord/src/main.rs");
        assert_eq!(result.file_changes[1].change_type, FileChangeType::Added);
    }
    
    #[test]
    fn test_parse_jsonl_multiple_commits() {
        let jsonl_output = r#"{"commit_id":"4788d6aa9868abcd","description":"","author":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T01:47:24+01:00"},"committer":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T02:06:57+01:00"}}
{"commit_id":"8410531198dabbcc","description":"Ping on pulled changes\n","author":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T01:30:58+01:00"},"committer":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T01:41:27+01:00"}}
{"commit_id":"7dee30d4d055eeff","description":"feat(ping-discord): Add revision parameter","author":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T01:15:47+01:00"},"committer":{"name":"Svein Ove Aas","email":"sveina@gmail.com","timestamp":"2025-07-04T01:21:05+01:00"}}"#;
        
        let diff_output = "M tools/ping-discord/src/main.rs\nA tools/ping-discord/src/parser.rs\nM pull.sh\nM tools/ping-discord/Cargo.toml";
        
        let result = parse_jj_jsonl_output(jsonl_output, diff_output).unwrap();
        
        assert_eq!(result.commits.len(), 3);
        
        // Check first commit (empty description)
        assert_eq!(result.commits[0].hash, "4788d6aa9868");
        assert_eq!(result.commits[0].description, "");
        
        // Check second commit
        assert_eq!(result.commits[1].hash, "8410531198da");
        assert_eq!(result.commits[1].description, "Ping on pulled changes");
        
        // Check third commit
        assert_eq!(result.commits[2].hash, "7dee30d4d055");
        assert_eq!(result.commits[2].description, "feat(ping-discord): Add revision parameter");
        
        // Check aggregated file changes
        assert_eq!(result.file_changes.len(), 4);
    }
    
    #[test]
    fn test_file_change_aggregation() {
        let changes = vec![
            FileChange { path: "file1.txt".to_string(), change_type: FileChangeType::Added },
            FileChange { path: "file1.txt".to_string(), change_type: FileChangeType::Modified },
            FileChange { path: "file2.txt".to_string(), change_type: FileChangeType::Added },
            FileChange { path: "file2.txt".to_string(), change_type: FileChangeType::Deleted },
            FileChange { path: "file3.txt".to_string(), change_type: FileChangeType::Modified },
        ];
        
        let result = aggregate_file_changes(changes);
        
        // file1.txt: Added then Modified -> Added
        // file2.txt: Added then Deleted -> removed entirely
        // file3.txt: Modified -> Modified
        
        assert_eq!(result.len(), 2);
        
        let file1 = result.iter().find(|c| c.path == "file1.txt").unwrap();
        assert_eq!(file1.change_type, FileChangeType::Added);
        
        let file3 = result.iter().find(|c| c.path == "file3.txt").unwrap();
        assert_eq!(file3.change_type, FileChangeType::Modified);
        
        // file2.txt should not be in result
        assert!(result.iter().find(|c| c.path == "file2.txt").is_none());
    }
}