package collaboration

import (
	"database/sql"
	"encoding/json"
	"net/http"
)

// Personal settings are account-scoped and never included in project membership.
func (s *Server) profileObject(q queryer, uid string) (Object, error) {
	var payload []byte
	err := q.QueryRow("SELECT value FROM settings WHERE name=?", "profile:"+uid).Scan(&payload)
	if err == sql.ErrNoRows {
		return Object{ID: "@personal", Project: "@personal", Kind: "profile", Name: "个人配置", Body: json.RawMessage(`{}`), Revision: 0, ModifiedBy: uid}, nil
	}
	if err != nil {
		return Object{}, err
	}
	plain, err := s.unseal(payload)
	if err != nil {
		return Object{}, err
	}
	var o Object
	err = json.Unmarshal(plain, &o)
	return o, err
}
func (s *Server) getProfile(w http.ResponseWriter, r *http.Request, u user) error {
	o, err := s.profileObject(s.db, u.ID)
	if err == nil {
		write(w, 200, o)
	}
	return err
}
func validProfile(body json.RawMessage) error {
	if len(body) > 1<<20 {
		return bad("个人配置不能超过 1 MiB")
	}
	var v map[string]json.RawMessage
	if json.Unmarshal(body, &v) != nil {
		return bad("个人配置格式无效")
	}
	for k, value := range v {
		switch k {
		case "appearance":
			var a map[string]any
			if json.Unmarshal(value, &a) != nil {
				return bad("界面配置格式无效")
			}
			for name := range a {
				switch name {
				case "scrollbackLines", "sshTerminalFontName", "sshTerminalFontSize", "sshServiceManagementEnabled", "sshDefaultFileViewMode", "sidebarVisible", "rdpOpenMode":
				default:
					return bad("个人配置包含不能同步的字段")
				}
			}
		case "connections":
			var c map[string]map[string]any
			if json.Unmarshal(value, &c) != nil || len(c) > 5000 {
				return bad("个人连接配置格式无效")
			}
			for _, options := range c {
				for name, value := range options {
					switch name {
					case "favorite", "hidden", "connectionColor", "starred", "color", "isHidden":
					default:
						return bad("个人连接配置包含不允许同步的字段")
					}
					switch value.(type) {
					case nil, bool, string, float64:
					default:
						return bad("个人连接配置值无效")
					}
				}
			}
		case "tables":
			var tables []json.RawMessage
			if json.Unmarshal(value, &tables) != nil || len(tables) > 1000 {
				return bad("表配置格式无效")
			}
		default:
			return bad("个人配置包含不允许同步的字段")
		}
	}
	return nil
}
func (s *Server) putProfile(w http.ResponseWriter, r *http.Request, u user) error {
	var v Mutation
	if err := decode(r, &v); err != nil {
		return err
	}
	if v.Kind != "profile" || !validID(v.Mutation) || v.Deleted {
		return bad("个人配置请求无效")
	}
	if err := validProfile(v.Body); err != nil {
		return err
	}
	var result Object
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		old, err := s.profileObject(tx, u.ID)
		if err != nil {
			return err
		}
		if old.Mutation == v.Mutation {
			result = old
			return nil
		}
		if old.Revision != v.Base {
			return conflict()
		}
		result = Object{ID: "@personal", Project: "@personal", Kind: "profile", Name: "个人配置", Body: v.Body, Bytes: len(v.Body), Revision: old.Revision + 1, Modified: now(), ModifiedBy: u.ID, Mutation: v.Mutation}
		encrypted, err := s.seal(jsonBytes(result))
		if err != nil {
			return err
		}
		if _, err = tx.Exec("INSERT INTO settings VALUES(?,?) ON CONFLICT(name) DO UPDATE SET value=excluded.value", "profile:"+u.ID, encrypted); err != nil {
			return err
		}
		return audit(tx, u.ID, "", "", "profile_saved")
	})
	if err == nil {
		write(w, 200, result)
	}
	return err
}
