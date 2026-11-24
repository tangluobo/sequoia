// Package collaboration implements the Tomato workspace collaboration protocol.
// No endpoint opens a database connection described by an uploaded object.
package collaboration

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	_ "modernc.org/sqlite"
)

const MaxObject = 8 << 20

type Config struct {
	DataDir      string
	Registration bool
	MaxObjects   int
	MaxBytes     int64
	SessionTTL   time.Duration
	Mailer       Mailer
	PublicURL    string
}

type Server struct {
	db       *sql.DB
	box      cipher.AEAD
	cfg      Config
	writes   sync.Mutex
	wakeMu   sync.Mutex
	wake     chan struct{}
	limits   sync.Mutex
	attempts map[string]attempt
}
type attempt struct {
	Count int
	Until time.Time
}

func Open(cfg Config) (*Server, error) {
	if cfg.DataDir == "" {
		return nil, errors.New("data directory is required")
	}
	if cfg.MaxObjects == 0 {
		cfg.MaxObjects = 5000
	}
	if cfg.MaxBytes == 0 {
		cfg.MaxBytes = 1 << 30
	}
	if cfg.SessionTTL == 0 {
		cfg.SessionTTL = 7 * 24 * time.Hour
	}
	if err := os.MkdirAll(cfg.DataDir, 0700); err != nil {
		return nil, err
	}
	keyPath := filepath.Join(cfg.DataDir, "encryption.key")
	key, err := os.ReadFile(keyPath)
	if errors.Is(err, os.ErrNotExist) {
		// Never silently regenerate a key for an existing database.
		if _, e := os.Stat(filepath.Join(cfg.DataDir, "cloud.db")); e == nil {
			return nil, errors.New("existing database has no encryption key; restore its key")
		}
		key = make([]byte, 32)
		if _, err = rand.Read(key); err != nil {
			return nil, err
		}
		f, e := os.OpenFile(keyPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if e != nil {
			return nil, e
		}
		_, err = f.Write(key)
		if err == nil {
			err = f.Sync()
		}
		closeErr := f.Close()
		if err == nil {
			err = closeErr
		}
	}
	if err != nil {
		return nil, err
	}
	if len(key) != 32 {
		return nil, errors.New("invalid encryption key")
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	box, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	path, err := filepath.Abs(filepath.Join(cfg.DataDir, "cloud.db"))
	if err != nil {
		return nil, err
	}
	db, err := sql.Open("sqlite", "file:"+filepath.ToSlash(path)+"?_pragma=foreign_keys(1)&_pragma=journal_mode(WAL)&_pragma=busy_timeout(5000)&_pragma=cache_size(-4096)")
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	s := &Server{db: db, box: box, cfg: cfg, wake: make(chan struct{}), attempts: make(map[string]attempt)}
	if _, err = db.Exec(schema); err != nil {
		db.Close()
		return nil, err
	}
	var verifiedColumn int
	if err = db.QueryRow("SELECT COUNT(*) FROM pragma_table_info('users') WHERE name='verified'").Scan(&verifiedColumn); err != nil {
		db.Close()
		return nil, err
	}
	if verifiedColumn == 0 {
		if _, err = db.Exec("ALTER TABLE users ADD COLUMN verified INTEGER NOT NULL DEFAULT 1"); err != nil {
			db.Close()
			return nil, err
		}
	}
	// Key validation also detects accidentally restoring a different key.
	var actorColumn int
	if err = db.QueryRow("SELECT COUNT(*) FROM pragma_table_info('events') WHERE name='actor_id'").Scan(&actorColumn); err != nil {
		db.Close()
		return nil, err
	}
	if actorColumn == 0 {
		if _, err = db.Exec("ALTER TABLE events ADD COLUMN actor_id TEXT NOT NULL DEFAULT ''; UPDATE events SET actor_id=user_id WHERE actor_id=''"); err != nil {
			db.Close()
			return nil, err
		}
	}
	var check []byte
	err = db.QueryRow("SELECT value FROM settings WHERE name='key-check'").Scan(&check)
	if errors.Is(err, sql.ErrNoRows) {
		check, err = s.seal([]byte("Tomato Cloud key v1"))
		if err == nil {
			_, err = db.Exec("INSERT INTO settings VALUES('key-check',?)", check)
		}
	} else if err == nil {
		var plain []byte
		plain, err = s.unseal(check)
		if err == nil && string(plain) != "Tomato Cloud key v1" {
			err = errors.New("encryption key mismatch")
		}
	}
	if err != nil {
		db.Close()
		return nil, fmt.Errorf("database key validation: %w", err)
	}
	return s, nil
}
func (s *Server) Close() error { return s.db.Close() }
func (s *Server) seal(b []byte) ([]byte, error) {
	nonce := make([]byte, s.box.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return nil, err
	}
	return s.box.Seal(nonce, nonce, b, nil), nil
}
func (s *Server) unseal(b []byte) ([]byte, error) {
	n := s.box.NonceSize()
	if len(b) < n {
		return nil, errors.New("corrupt encrypted object")
	}
	return s.box.Open(nil, b[:n], b[n:], nil)
}
func id() string {
	b := make([]byte, 16)
	_, err := rand.Read(b)
	if err != nil {
		panic(err)
	}
	return hex.EncodeToString(b)
}
func now() int64 { return time.Now().Unix() }
func (s *Server) notify() {
	s.wakeMu.Lock()
	close(s.wake)
	s.wake = make(chan struct{})
	s.wakeMu.Unlock()
}
func (s *Server) listener() <-chan struct{} { s.wakeMu.Lock(); defer s.wakeMu.Unlock(); return s.wake }
func audit(tx *sql.Tx, uid, pid, oid, action string) error {
	return auditFor(tx, uid, uid, pid, oid, action)
}
func auditFor(tx *sql.Tx, actor, recipient, pid, oid, action string) error {
	_, err := tx.Exec("INSERT INTO events(project_id,user_id,object_id,action,created,actor_id) VALUES(?,?,?,?,?,?)", pid, recipient, oid, action, now(), actor)
	return err
}
func (s *Server) transaction(ctx context.Context, fn func(*sql.Tx) error) error {
	s.writes.Lock()
	defer s.writes.Unlock()
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if err = fn(tx); err != nil {
		return err
	}
	if err = tx.Commit(); err == nil {
		s.notify()
	}
	return err
}
func jsonBytes(v any) []byte { b, _ := json.Marshal(v); return b }

const schema = `
CREATE TABLE IF NOT EXISTS settings(name TEXT PRIMARY KEY,value BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS users(id TEXT PRIMARY KEY,email TEXT UNIQUE NOT NULL,name TEXT NOT NULL,password TEXT NOT NULL,admin INTEGER NOT NULL DEFAULT 0,disabled INTEGER NOT NULL DEFAULT 0,mfa BLOB,mfa_last INTEGER NOT NULL DEFAULT 0,created INTEGER NOT NULL,verified INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS account_tokens(hash TEXT PRIMARY KEY,user_id TEXT NOT NULL,purpose TEXT NOT NULL,expires INTEGER NOT NULL,created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS recovery(user_id TEXT NOT NULL,hash TEXT NOT NULL,PRIMARY KEY(user_id,hash));
CREATE TABLE IF NOT EXISTS sessions(hash TEXT PRIMARY KEY,user_id TEXT NOT NULL REFERENCES users(id),device TEXT NOT NULL,created INTEGER NOT NULL,expires INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY,name TEXT NOT NULL,owner TEXT NOT NULL REFERENCES users(id),revision INTEGER NOT NULL DEFAULT 1,created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS members(project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,user_id TEXT NOT NULL REFERENCES users(id),role TEXT NOT NULL,PRIMARY KEY(project_id,user_id));
CREATE TABLE IF NOT EXISTS invitations(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,email TEXT NOT NULL,role TEXT NOT NULL,invited_by TEXT NOT NULL,created INTEGER NOT NULL,UNIQUE(project_id,email));
CREATE TABLE IF NOT EXISTS objects(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,parent_id TEXT NOT NULL DEFAULT '',kind TEXT NOT NULL,name TEXT NOT NULL,body BLOB NOT NULL,bytes INTEGER NOT NULL,revision INTEGER NOT NULL,deleted INTEGER NOT NULL DEFAULT 0,modified INTEGER NOT NULL,modified_by TEXT NOT NULL,mutation_id TEXT NOT NULL DEFAULT '');
CREATE INDEX IF NOT EXISTS object_project ON objects(project_id,deleted,id);
CREATE TABLE IF NOT EXISTS versions(object_id TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,revision INTEGER NOT NULL,parent_id TEXT NOT NULL,kind TEXT NOT NULL,name TEXT NOT NULL,body BLOB NOT NULL,bytes INTEGER NOT NULL,deleted INTEGER NOT NULL,modified INTEGER NOT NULL,modified_by TEXT NOT NULL,PRIMARY KEY(object_id,revision));
CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,project_id TEXT NOT NULL,user_id TEXT NOT NULL,object_id TEXT NOT NULL,action TEXT NOT NULL,created INTEGER NOT NULL,actor_id TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS event_project ON events(project_id,seq);
PRAGMA user_version=1;
`
