package collaboration

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
)

// Backup uses SQLite's online snapshot, never copies an open WAL database file.
// Restore requires both cloud.db and encryption.key from this directory.
func (s *Server) Backup(directory string) error {
	if directory == "" {
		return errors.New("a new backup directory is required")
	}
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return err
	}
	if err = os.Mkdir(absolute, 0700); err != nil {
		return err
	}
	s.writes.Lock()
	defer s.writes.Unlock()
	dbfile := filepath.Join(absolute, "cloud.db")
	if _, err = s.db.Exec("VACUUM INTO '" + strings.ReplaceAll(filepath.ToSlash(dbfile), "'", "''") + "'"); err != nil {
		return err
	}
	key, err := os.ReadFile(filepath.Join(s.cfg.DataDir, "encryption.key"))
	if err != nil {
		return err
	}
	if err = os.WriteFile(filepath.Join(absolute, "encryption.key"), key, 0600); err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(absolute, "README.txt"), []byte("Stop Tomato Cloud; restore cloud.db and encryption.key together to a new private data directory. Never restore only the database. Keep this backup private.\n"), 0600)
}
