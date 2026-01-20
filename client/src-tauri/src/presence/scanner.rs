//! Process scanner for detecting running games/applications.

use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use super::{GameEntry, GamesDatabase};

/// Scanner for detecting running games.
pub struct ProcessScanner {
    system: System,
    pub games_db: GamesDatabase,
}

impl ProcessScanner {
    /// Create a new process scanner.
    pub fn new() -> Self {
        Self {
            system: System::new_with_specifics(
                RefreshKind::new().with_processes(ProcessRefreshKind::new())
            ),
            games_db: GamesDatabase::load(),
        }
    }

    /// Refresh process list and find matching game.
    /// Returns the first matching game found.
    pub fn scan(&mut self) -> Option<GameEntry> {
        self.system.refresh_processes();

        for process in self.system.processes().values() {
            let name = process.name();
            if let Some(game) = self.games_db.find_by_process(name) {
                return Some(game.clone());
            }
        }
        None
    }

    /// Scan and return all detected games (not just first).
    pub fn scan_all(&mut self) -> Vec<GameEntry> {
        self.system.refresh_processes();

        let mut found = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for process in self.system.processes().values() {
            let name = process.name();
            if let Some(game) = self.games_db.find_by_process(name) {
                if seen_names.insert(game.name.clone()) {
                    found.push(game.clone());
                }
            }
        }
        found
    }
}

impl Default for ProcessScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scanner_creation() {
        let scanner = ProcessScanner::new();
        assert!(!scanner.games_db.games.is_empty());
    }

    #[test]
    fn test_scanner_scan_runs() {
        let mut scanner = ProcessScanner::new();
        // Just verify scan doesn't panic - actual results depend on running processes
        let _ = scanner.scan();
    }

    #[test]
    fn test_scanner_scan_all_runs() {
        let mut scanner = ProcessScanner::new();
        // Just verify scan_all doesn't panic
        let results = scanner.scan_all();
        // Results are system-dependent, just ensure we got a valid vec
        drop(results);
    }
}
