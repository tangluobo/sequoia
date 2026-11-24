package collaboration

import (
	"database/sql"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"
)

type Object struct {
	ID         string          `json:"id"`
	Project    string          `json:"project_id"`
	Parent     string          `json:"parent_id"`
	Kind       string          `json:"kind"`
	Name       string          `json:"name"`
	Body       json.RawMessage `json:"body,omitempty"`
	Bytes      int             `json:"bytes"`
	Revision   int64           `json:"revision"`
	Deleted    bool            `json:"deleted"`
	Modified   int64           `json:"modified"`
	ModifiedBy string          `json:"modified_by"`
	Mutation   string          `json:"mutation_id,omitempty"`
}
type Mutation struct {
	Parent   string          `json:"parent_id"`
	Kind     string          `json:"kind"`
	Name     string          `json:"name"`
	Body     json.RawMessage `json:"body"`
	Base     int64           `json:"base_revision"`
	Mutation string          `json:"mutation_id"`
	Deleted  bool            `json:"deleted"`
}

const objectColumns = "id,project_id,parent_id,kind,name,body,bytes,revision,deleted,modified,modified_by,mutation_id"

type scanner interface{ Scan(...any) error }

func (s *Server) scan(row scanner) (Object, error) {
	var o Object
	var encrypted []byte
	err := row.Scan(&o.ID, &o.Project, &o.Parent, &o.Kind, &o.Name, &encrypted, &o.Bytes, &o.Revision, &o.Deleted, &o.Modified, &o.ModifiedBy, &o.Mutation)
	if err == nil {
		o.Body, err = s.unseal(encrypted)
	}
	return o, err
}
func kindValid(kind string) bool {
	switch kind {
	case "connection", "query", "snippet", "pipeline", "model", "bi", "group", "profile":
		return true
	}
	return false
}

// Connection documents deliberately use an allow-list. Unknown local properties
// (including nested credentials and URL connection strings) cannot become cloud data.
var connectionKeys = map[string]bool{"type": true, "host": true, "port": true, "username": true, "user": true, "database": true, "schema": true, "charset": true, "encoding": true, "sslMode": true, "ssl": true, "timeout": true, "comment": true, "databaseTls": true, "databaseProfile": true, "connectTimeout": true, "socketTimeout": true, "queryRowLimit": true, "useSshTunnel": true, "sshTunnelHost": true, "sshTunnelPort": true, "sshTunnelUsername": true, "sshTunnelUsePassword": true, "sshTunnelUseKey": true}

func validBody(kind string, body json.RawMessage) error {
	if len(body) > MaxObject || !json.Valid(body) {
		return bad("对象必须是有效 JSON，最大 8 MiB")
	}
	if kind == "connection" {
		var value map[string]any
		if json.Unmarshal(body, &value) != nil || value == nil {
			return bad("连接设置格式无效")
		}
		for key, v := range value {
			if !connectionKeys[key] {
				return bad("连接设置包含不允许同步的属性：" + key)
			}
			switch value := v.(type) {
			case nil, bool, float64:
			case string:
				if strings.ContainsAny(value, "\r\n\x00") || len(value) > 1000 || strings.Contains(value, "://") {
					return bad("连接属性包含无效值")
				}
			default:
				return bad("连接设置仅允许普通标量属性")
			}
		}
	}
	return nil
}
func (s *Server) objects(w http.ResponseWriter, r *http.Request, u user) error {
	pid := r.PathValue("project")
	if err := require(s.db, pid, u.ID, false, false); err != nil {
		return err
	}
	rows, err := s.db.QueryContext(r.Context(), "SELECT id,project_id,parent_id,kind,name,bytes,revision,deleted,modified,modified_by FROM objects WHERE project_id=? AND (deleted=0 OR ?=1) AND id>? ORDER BY id LIMIT ?", pid, r.URL.Query().Get("include_deleted") == "1", r.URL.Query().Get("after"), limit(r))
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []Object{}
	for rows.Next() {
		var o Object
		if err = rows.Scan(&o.ID, &o.Project, &o.Parent, &o.Kind, &o.Name, &o.Bytes, &o.Revision, &o.Deleted, &o.Modified, &o.ModifiedBy); err != nil {
			return err
		}
		items = append(items, o)
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}
func (s *Server) getObject(w http.ResponseWriter, r *http.Request, u user) error {
	pid := r.PathValue("project")
	if err := require(s.db, pid, u.ID, false, false); err != nil {
		return err
	}
	var o Object
	var err error
	if revision := r.URL.Query().Get("revision"); revision != "" {
		rev, e := strconv.ParseInt(revision, 10, 64)
		if e != nil {
			return bad("版本无效")
		}
		var encrypted []byte
		err = s.db.QueryRowContext(r.Context(), "SELECT v.object_id,o.project_id,v.parent_id,v.kind,v.name,v.body,v.bytes,v.revision,v.deleted,v.modified,v.modified_by FROM versions v JOIN objects o ON o.id=v.object_id WHERE o.project_id=? AND o.id=? AND v.revision=?", pid, r.PathValue("object"), rev).Scan(&o.ID, &o.Project, &o.Parent, &o.Kind, &o.Name, &encrypted, &o.Bytes, &o.Revision, &o.Deleted, &o.Modified, &o.ModifiedBy)
		if err == nil {
			o.Body, err = s.unseal(encrypted)
		}
	} else {
		o, err = s.scan(s.db.QueryRowContext(r.Context(), "SELECT "+objectColumns+" FROM objects WHERE project_id=? AND id=?", pid, r.PathValue("object")))
	}
	if err == sql.ErrNoRows {
		return missing()
	}
	if err != nil {
		return err
	}
	w.Header().Set("ETag", strconv.FormatInt(o.Revision, 10))
	write(w, 200, o)
	return nil
}
func validID(id string) bool {
	if len(id) < 1 || len(id) > 80 {
		return false
	}
	for _, ch := range id {
		if !(ch >= 'a' && ch <= 'z' || ch >= 'A' && ch <= 'Z' || ch >= '0' && ch <= '9' || ch == '-' || ch == '_') {
			return false
		}
	}
	return true
}
func (s *Server) quota(tx *sql.Tx, pid string, extraBytes int, extraCount int) error {
	var n int
	var bytes int64
	if err := tx.QueryRow("SELECT COUNT(*),COALESCE(SUM(o.bytes),0) FROM objects o JOIN projects p ON p.id=o.project_id WHERE p.owner=(SELECT owner FROM projects WHERE id=?) AND o.deleted=0", pid).Scan(&n, &bytes); err != nil {
		return err
	}
	if n+extraCount > s.cfg.MaxObjects || bytes+int64(extraBytes) > s.cfg.MaxBytes {
		return fault{413, "quota", "云端对象或空间达到上限，本地修改已保留"}
	}
	return nil
}
func (s *Server) parent(tx *sql.Tx, pid, oid, parent string) error {
	visited := map[string]bool{oid: true}
	for depth := 0; parent != ""; depth++ {
		if depth >= 64 || visited[parent] {
			return bad("对象分组存在循环或超过 64 层")
		}
		visited[parent] = true
		var next, kind string
		if tx.QueryRow("SELECT parent_id,kind FROM objects WHERE id=? AND project_id=? AND deleted=0", parent, pid).Scan(&next, &kind) != nil {
			return bad("父对象不存在")
		}
		if kind != "connection" && kind != "group" {
			return bad("父对象必须是连接或分组")
		}
		parent = next
	}
	return nil
}
func (s *Server) storeObject(tx *sql.Tx, o Object) error {
	encrypted, err := s.seal(o.Body)
	if err != nil {
		return err
	}
	_, err = tx.Exec("INSERT INTO objects VALUES(?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,parent_id=excluded.parent_id,kind=excluded.kind,name=excluded.name,body=excluded.body,bytes=excluded.bytes,revision=excluded.revision,deleted=excluded.deleted,modified=excluded.modified,modified_by=excluded.modified_by,mutation_id=excluded.mutation_id", o.ID, o.Project, o.Parent, o.Kind, o.Name, encrypted, o.Bytes, o.Revision, o.Deleted, o.Modified, o.ModifiedBy, o.Mutation)
	if err != nil {
		return err
	}
	_, err = tx.Exec("INSERT INTO versions VALUES(?,?,?,?,?,?,?,?,?,?)", o.ID, o.Revision, o.Parent, o.Kind, o.Name, encrypted, o.Bytes, o.Deleted, o.Modified, o.ModifiedBy)
	if err != nil {
		return err
	}
	_, err = tx.Exec("DELETE FROM versions WHERE object_id=? AND revision<=?", o.ID, o.Revision-50)
	return err
}
func (s *Server) mutate(tx *sql.Tx, uid, pid, oid string, v Mutation) (Object, error) {
	if err := require(tx, pid, uid, true, false); err != nil {
		return Object{}, err
	}
	if !validID(oid) || !validID(v.Mutation) || !validName(v.Name) || !kindValid(v.Kind) {
		return Object{}, bad("对象名称、类型或标识无效")
	}
	if err := validBody(v.Kind, v.Body); err != nil {
		return Object{}, err
	}
	old, err := s.scan(tx.QueryRow("SELECT "+objectColumns+" FROM objects WHERE id=?", oid))
	exists := err == nil
	if err != nil && err != sql.ErrNoRows {
		return Object{}, err
	}
	if exists && old.Project != pid {
		return Object{}, conflict()
	}
	if exists && old.Mutation == v.Mutation {
		return old, nil
	}
	if (exists && old.Revision != v.Base) || (!exists && v.Base != 0) {
		return Object{}, conflict()
	}
	if exists && old.Kind != v.Kind {
		return Object{}, bad("对象类型不能改变")
	}
	if err := s.parent(tx, pid, oid, v.Parent); err != nil {
		return Object{}, err
	}
	if v.Deleted {
		var n int
		if err := tx.QueryRow("SELECT COUNT(*) FROM objects WHERE parent_id=? AND deleted=0", oid).Scan(&n); err != nil {
			return Object{}, err
		}
		if n > 0 {
			return Object{}, bad("请先删除子对象")
		}
	}
	extraCount := 1
	extraBytes := len(v.Body)
	if v.Deleted {
		extraCount = 0
		extraBytes = 0
	}
	if exists && !old.Deleted {
		extraCount--
		extraBytes -= old.Bytes
	}
	if err := s.quota(tx, pid, extraBytes, extraCount); err != nil {
		return Object{}, err
	}
	o := Object{ID: oid, Project: pid, Parent: v.Parent, Kind: v.Kind, Name: v.Name, Body: v.Body, Bytes: len(v.Body), Revision: v.Base + 1, Deleted: v.Deleted, Modified: now(), ModifiedBy: uid, Mutation: v.Mutation}
	if err := s.storeObject(tx, o); err != nil {
		return Object{}, err
	}
	action := "object_saved"
	if v.Deleted {
		action = "object_deleted"
	}
	return o, audit(tx, uid, pid, oid, action)
}
func (s *Server) putObject(w http.ResponseWriter, r *http.Request, u user) error {
	var v Mutation
	if err := decode(r, &v); err != nil {
		return err
	}
	var o Object
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		var err error
		o, err = s.mutate(tx, u.ID, r.PathValue("project"), r.PathValue("object"), v)
		return err
	})
	if err == nil {
		write(w, 200, o)
	}
	return err
}
func (s *Server) deleteObject(w http.ResponseWriter, r *http.Request, u user) error {
	base, e := strconv.ParseInt(r.Header.Get("If-Match"), 10, 64)
	if e != nil {
		return bad("删除需要 If-Match 版本")
	}
	var o Object
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		pid, oid := r.PathValue("project"), r.PathValue("object")
		if err := require(tx, pid, u.ID, true, false); err != nil {
			return err
		}
		old, err := s.scan(tx.QueryRow("SELECT "+objectColumns+" FROM objects WHERE project_id=? AND id=?", pid, oid))
		if err != nil {
			return missing()
		}
		o, err = s.mutate(tx, u.ID, pid, oid, Mutation{old.Parent, old.Kind, old.Name, old.Body, base, id(), true})
		return err
	})
	if err == nil {
		write(w, 200, o)
	}
	return err
}
func (s *Server) history(w http.ResponseWriter, r *http.Request, u user) error {
	pid := r.PathValue("project")
	if err := require(s.db, pid, u.ID, false, false); err != nil {
		return err
	}
	rows, err := s.db.QueryContext(r.Context(), "SELECT v.revision,v.name,v.bytes,v.deleted,v.modified,v.modified_by FROM versions v JOIN objects o ON o.id=v.object_id WHERE o.project_id=? AND o.id=? ORDER BY v.revision DESC LIMIT 50", pid, r.PathValue("object"))
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []map[string]any{}
	for rows.Next() {
		var rev, bytes, t int64
		var n, uid string
		var d bool
		if err = rows.Scan(&rev, &n, &bytes, &d, &t, &uid); err != nil {
			return err
		}
		items = append(items, map[string]any{"revision": rev, "name": n, "bytes": bytes, "deleted": d, "modified": t, "modified_by": uid})
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}

func (s *Server) transfer(w http.ResponseWriter, r *http.Request, u user) error {
	var v struct {
		Objects []string `json:"objects"`
		Target  string   `json:"target"`
		Copy    bool     `json:"copy"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if len(v.Objects) < 1 || len(v.Objects) > 200 || v.Target == r.PathValue("project") {
		return bad("请选择其他目标项目和 1–200 个对象")
	}
	var result []Object
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		source := r.PathValue("project")
		if err := require(tx, source, u.ID, !v.Copy, false); err != nil {
			return err
		}
		if err := require(tx, v.Target, u.ID, true, false); err != nil {
			return err
		}
		seen := map[string]bool{}
		pending := append([]string{}, v.Objects...)
		selected := []Object{}
		bytes := 0
		for len(pending) > 0 {
			oid := pending[0]
			pending = pending[1:]
			if seen[oid] {
				continue
			}
			seen[oid] = true
			if len(seen) > 5000 {
				return bad("传输对象过多")
			}
			o, err := s.scan(tx.QueryRow("SELECT "+objectColumns+" FROM objects WHERE project_id=? AND id=? AND deleted=0", source, oid))
			if err != nil {
				return missing()
			}
			bytes += o.Bytes
			if bytes > 32<<20 {
				return bad("单次传输超过 32 MiB，请分批")
			}
			selected = append(selected, o)
			rows, err := tx.Query("SELECT id FROM objects WHERE project_id=? AND parent_id=? AND deleted=0", source, oid)
			if err != nil {
				return err
			}
			for rows.Next() {
				var child string
				if err = rows.Scan(&child); err != nil {
					rows.Close()
					return err
				}
				pending = append(pending, child)
			}
			if err = rows.Err(); err != nil {
				rows.Close()
				return err
			}
			rows.Close()
		}
		if err := s.quota(tx, v.Target, bytes, len(selected)); err != nil {
			return err
		}
		mapping := map[string]string{}
		for _, o := range selected {
			mapping[o.ID] = id()
		}
		// Copies receive fresh IDs; moves are copy+tombstone in one transaction so
		// clients of the source see removals instead of losing their event history.
		for _, o := range selected {
			next := o
			next.ID = mapping[o.ID]
			next.Parent = mapping[o.Parent]
			next.Project = v.Target
			next.Revision = 1
			next.Modified = now()
			next.ModifiedBy = u.ID
			next.Mutation = id()
			if err := s.storeObject(tx, next); err != nil {
				return err
			}
			if err := audit(tx, u.ID, v.Target, next.ID, "object_saved"); err != nil {
				return err
			}
			next.Body = nil
			result = append(result, next)
		}
		if !v.Copy {
			for _, o := range selected {
				o.Revision++
				o.Deleted = true
				o.Modified = now()
				o.ModifiedBy = u.ID
				o.Mutation = id()
				if err := s.storeObject(tx, o); err != nil {
					return err
				}
				if err := audit(tx, u.ID, source, o.ID, "object_deleted"); err != nil {
					return err
				}
			}
		}
		return nil
	})
	if err == nil {
		write(w, 200, result)
	}
	return err
}
