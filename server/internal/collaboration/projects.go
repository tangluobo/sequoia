package collaboration

import (
	"database/sql"
	"net/http"
	"strings"
)

type queryer interface{ QueryRow(string, ...any) *sql.Row }

func role(q queryer, pid, uid string) (string, error) {
	var value string
	if q.QueryRow("SELECT role FROM members WHERE project_id=? AND user_id=?", pid, uid).Scan(&value) != nil {
		return "", missing()
	}
	return value, nil
}
func require(q queryer, pid, uid string, write, manage bool) error {
	v, err := role(q, pid, uid)
	if err != nil {
		return err
	}
	if (write && v == "viewer") || (manage && v != "owner" && v != "manager") {
		return denied()
	}
	return nil
}
func validRole(r string) bool { return r == "manager" || r == "editor" || r == "viewer" }

type project struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	Owner    string `json:"owner"`
	Role     string `json:"role"`
	Revision int64  `json:"revision"`
	Created  int64  `json:"created"`
}

func (s *Server) projects(w http.ResponseWriter, r *http.Request, u user) error {
	rows, err := s.db.QueryContext(r.Context(), "SELECT p.id,p.name,p.owner,m.role,p.revision,p.created FROM projects p JOIN members m ON m.project_id=p.id WHERE m.user_id=? AND p.id>? ORDER BY p.id LIMIT ?", u.ID, r.URL.Query().Get("after"), limit(r))
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []project{}
	for rows.Next() {
		var p project
		if err = rows.Scan(&p.ID, &p.Name, &p.Owner, &p.Role, &p.Revision, &p.Created); err != nil {
			return err
		}
		items = append(items, p)
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}
func validName(v string) bool {
	return len(strings.TrimSpace(v)) > 0 && len(v) <= 240 && !strings.ContainsAny(v, "\x00\r\n")
}
func (s *Server) createProject(w http.ResponseWriter, r *http.Request, u user) error {
	var v struct {
		Name string `json:"name"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if !validName(v.Name) {
		return bad("请输入项目名称（最多 240 字节）")
	}
	p := project{ID: id(), Name: v.Name, Owner: u.ID, Role: "owner", Revision: 1, Created: now()}
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		var count int
		if err := tx.QueryRow("SELECT COUNT(*) FROM projects WHERE owner=?", u.ID).Scan(&count); err != nil {
			return err
		}
		if count >= 500 {
			return fault{413, "quota", "项目数量已达到上限"}
		}
		if _, err := tx.Exec("INSERT INTO projects VALUES(?,?,?,?,?)", p.ID, p.Name, p.Owner, p.Revision, p.Created); err != nil {
			return err
		}
		if _, err := tx.Exec("INSERT INTO members VALUES(?,?, 'owner')", p.ID, u.ID); err != nil {
			return err
		}
		return audit(tx, u.ID, p.ID, "", "project_created")
	})
	if err == nil {
		write(w, 201, p)
	}
	return err
}
func (s *Server) renameProject(w http.ResponseWriter, r *http.Request, u user) error {
	var v struct {
		Name     string `json:"name"`
		Revision int64  `json:"revision"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if !validName(v.Name) {
		return bad("项目名称无效")
	}
	pid := r.PathValue("project")
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		if err := require(tx, pid, u.ID, true, true); err != nil {
			return err
		}
		result, err := tx.Exec("UPDATE projects SET name=?,revision=revision+1 WHERE id=? AND revision=?", v.Name, pid, v.Revision)
		if err != nil {
			return err
		}
		n, _ := result.RowsAffected()
		if n != 1 {
			return conflict()
		}
		return audit(tx, u.ID, pid, "", "project_renamed")
	})
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
func (s *Server) deleteProject(w http.ResponseWriter, r *http.Request, u user) error {
	pid := r.PathValue("project")
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		value, err := role(tx, pid, u.ID)
		if err != nil {
			return err
		}
		if value != "owner" {
			return denied()
		}
		var count int
		if err = tx.QueryRow("SELECT COUNT(*) FROM objects WHERE project_id=? AND deleted=0", pid).Scan(&count); err != nil {
			return err
		}
		if count > 0 {
			return bad("请先删除项目中的全部对象")
		}
		rows, err := tx.Query("SELECT user_id FROM members WHERE project_id=?", pid)
		if err != nil {
			return err
		}
		users := []string{}
		for rows.Next() {
			var uid string
			if err = rows.Scan(&uid); err != nil {
				rows.Close()
				return err
			}
			users = append(users, uid)
		}
		if err = rows.Err(); err != nil {
			rows.Close()
			return err
		}
		rows.Close()
		for _, uid := range users {
			if err = auditFor(tx, u.ID, uid, pid, "", "project_removed"); err != nil {
				return err
			}
		}
		_, err = tx.Exec("DELETE FROM projects WHERE id=?", pid)
		return err
	})
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
func (s *Server) members(w http.ResponseWriter, r *http.Request, u user) error {
	pid := r.PathValue("project")
	if err := require(s.db, pid, u.ID, false, false); err != nil {
		return err
	}
	rows, err := s.db.QueryContext(r.Context(), "SELECT u.id,u.email,u.name,m.role FROM members m JOIN users u ON u.id=m.user_id WHERE m.project_id=? ORDER BY u.email LIMIT 500", pid)
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []map[string]any{}
	for rows.Next() {
		var id, e, n, role string
		if err = rows.Scan(&id, &e, &n, &role); err != nil {
			return err
		}
		items = append(items, map[string]any{"id": id, "email": e, "name": n, "role": role})
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}
func (s *Server) setMember(w http.ResponseWriter, r *http.Request, u user) error {
	var v struct {
		Role string `json:"role"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if !validRole(v.Role) {
		return bad("成员权限无效")
	}
	pid, uid := r.PathValue("project"), r.PathValue("member")
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		if err := require(tx, pid, u.ID, true, true); err != nil {
			return err
		}
		current, err := role(tx, pid, uid)
		if err != nil {
			return err
		}
		if current == "owner" {
			return denied()
		}
		if _, err = tx.Exec("UPDATE members SET role=? WHERE project_id=? AND user_id=?", v.Role, pid, uid); err != nil {
			return err
		}
		return auditFor(tx, u.ID, uid, pid, "", "member_role_changed")
	})
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
func (s *Server) removeMember(w http.ResponseWriter, r *http.Request, u user) error {
	pid, uid := r.PathValue("project"), r.PathValue("member")
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		if uid != u.ID {
			if err := require(tx, pid, u.ID, true, true); err != nil {
				return err
			}
		}
		value, err := role(tx, pid, uid)
		if err != nil {
			return err
		}
		if value == "owner" {
			return bad("所有者不能退出项目")
		}
		if _, err = tx.Exec("DELETE FROM members WHERE project_id=? AND user_id=?", pid, uid); err != nil {
			return err
		}
		return auditFor(tx, u.ID, uid, pid, "", "member_removed")
	})
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
func (s *Server) invite(w http.ResponseWriter, r *http.Request, u user) error {
	var v struct {
		Email string `json:"email"`
		Role  string `json:"role"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	email, err := emailAddress(v.Email)
	if err != nil {
		return err
	}
	if !validRole(v.Role) {
		return bad("成员权限无效")
	}
	pid := r.PathValue("project")
	inv := id()
	err = s.transaction(r.Context(), func(tx *sql.Tx) error {
		if err := require(tx, pid, u.ID, true, true); err != nil {
			return err
		}
		var n int
		if err := tx.QueryRow("SELECT (SELECT COUNT(*) FROM members WHERE project_id=?)+(SELECT COUNT(*) FROM invitations WHERE project_id=?)", pid, pid).Scan(&n); err != nil {
			return err
		}
		if n >= 500 {
			return fault{413, "quota", "成员数量已达到上限"}
		}
		if _, err := tx.Exec("INSERT INTO invitations VALUES(?,?,?,?,?,?) ON CONFLICT(project_id,email) DO UPDATE SET role=excluded.role,invited_by=excluded.invited_by", inv, pid, email, v.Role, u.ID, now()); err != nil {
			return err
		}
		return audit(tx, u.ID, pid, "", "member_invited")
	})
	if err == nil {
		write(w, 201, map[string]bool{"invited": true})
	}
	return err
}
func (s *Server) invitations(w http.ResponseWriter, r *http.Request, u user) error {
	rows, err := s.db.QueryContext(r.Context(), "SELECT i.id,i.project_id,p.name,i.role,i.email,i.created FROM invitations i JOIN projects p ON p.id=i.project_id WHERE i.email=? OR i.project_id IN(SELECT project_id FROM members WHERE user_id=? AND role IN ('owner','manager')) ORDER BY i.id LIMIT 500", u.Email, u.ID)
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []map[string]any{}
	for rows.Next() {
		var id, p, n, role, e string
		var c int64
		if err = rows.Scan(&id, &p, &n, &role, &e, &c); err != nil {
			return err
		}
		items = append(items, map[string]any{"id": id, "project_id": p, "project": n, "role": role, "email": e, "created": c})
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}
func (s *Server) accept(w http.ResponseWriter, r *http.Request, u user) error {
	var v struct {
		Accept bool `json:"accept"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		var pid, role, email string
		if err := tx.QueryRow("SELECT project_id,role,email FROM invitations WHERE id=?", r.PathValue("invitation")).Scan(&pid, &role, &email); err != nil {
			return missing()
		}
		if email != u.Email {
			return denied()
		}
		if v.Accept {
			if _, err := tx.Exec("INSERT INTO members VALUES(?,?,?) ON CONFLICT(project_id,user_id) DO NOTHING", pid, u.ID, role); err != nil {
				return err
			}
		}
		if _, err := tx.Exec("DELETE FROM invitations WHERE id=?", r.PathValue("invitation")); err != nil {
			return err
		}
		return audit(tx, u.ID, pid, "", "invitation_answered")
	})
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
func (s *Server) cancelInvitation(w http.ResponseWriter, r *http.Request, u user) error {
	pid := r.PathValue("project")
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		if err := require(tx, pid, u.ID, true, true); err != nil {
			return err
		}
		_, err := tx.Exec("DELETE FROM invitations WHERE id=? AND project_id=?", r.PathValue("invitation"), pid)
		return err
	})
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
