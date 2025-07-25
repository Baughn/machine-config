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


// Serde structs for parsing jj json(self) output
#[derive(Debug, Deserialize)]
pub struct JJCommit {
    pub commit_id: String,
    pub description: String,
    pub author: JJSignature,
}

#[derive(Debug, Deserialize)]
pub struct JJSignature {
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

pub fn parse_file_changes(diff_output: &str) -> Vec<FileChange> {
    let mut file_changes = Vec::new();
    for line in diff_output.lines() {
        if let Some(change) = FileChange::from_status_line(line) {
            file_changes.push(change);
        }
    }
    file_changes
}

pub fn parse_commits_only(jsonl_output: &str) -> Result<Vec<Commit>, String> {
    let mut commits = Vec::new();
    
    // Parse JSONL (JSON Lines) format
    for line in jsonl_output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        
        let jj_commit: JJCommit = serde_json::from_str(line)
            .map_err(|e| format!("Failed to parse JSON line: {line} - Error: {e}"))?;
        
        // Convert to our Commit struct
        let commit = Commit {
            hash: jj_commit.commit_id[..12.min(jj_commit.commit_id.len())].to_string(), // Use first 12 chars like short hash
            author: jj_commit.author.email,
            timestamp: jj_commit.author.timestamp[..19].replace('T', " "), // Convert ISO to readable format
            description: jj_commit.description.trim().to_string(),
        };
        
        commits.push(commit);
    }
    
    Ok(commits)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_commits_only() {
        let jsonl_output = r#"{"commit_id":"7dee30d4d055f7b38e7a0328271e7cb51a76a9b1","description":"feat(ping-discord): Add revision parameter","author":{"email":"sveina@gmail.com","timestamp":"2025-07-04T01:15:47+01:00"}}"#;
        
        let result = parse_commits_only(jsonl_output).unwrap();
        
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].hash, "7dee30d4d055");
        assert_eq!(result[0].author, "sveina@gmail.com");
        assert_eq!(result[0].description, "feat(ping-discord): Add revision parameter");
        assert_eq!(result[0].timestamp, "2025-07-04 01:15:47");
    }
    
    #[test]
    fn test_parse_file_changes() {
        let diff_output = "M tools/ping-discord/Cargo.toml\nA tools/ping-discord/src/main.rs\nD old-file.txt";
        
        let result = parse_file_changes(diff_output);
        
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].path, "tools/ping-discord/Cargo.toml");
        assert_eq!(result[0].change_type, FileChangeType::Modified);
        assert_eq!(result[1].path, "tools/ping-discord/src/main.rs");
        assert_eq!(result[1].change_type, FileChangeType::Added);
        assert_eq!(result[2].path, "old-file.txt");
        assert_eq!(result[2].change_type, FileChangeType::Deleted);
    }
}