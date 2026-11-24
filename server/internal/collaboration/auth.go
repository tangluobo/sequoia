package collaboration

import (
	"crypto/hmac"
	"crypto/pbkdf2"
	"crypto/rand"
	"crypto/sha1"
	"crypto/sha256"
	"crypto/subtle"
	"database/sql"
	"encoding/base32"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"net/http"
	"net/mail"
	"strings"
	"time"
)

func passwordHash(password string) (string, error) {
	salt := make([]byte, 16)
	if _, err := rand.Read(salt); err != nil {
		return "", err
	}
	key, err := pbkdf2.Key(sha256.New, password, salt, 600000, 32)
	return hex.EncodeToString(salt) + ":" + hex.EncodeToString(key), err
}
func passwordOK(encoded, password string) bool {
	saltText, keyText, ok := strings.Cut(encoded, ":")
	if !ok {
		return false
	}
	salt, e := hex.DecodeString(saltText)
	if e != nil {
		return false
	}
	key, e := pbkdf2.Key(sha256.New, password, salt, 600000, 32)
	return e == nil && subtle.ConstantTimeCompare([]byte(hex.EncodeToString(key)), []byte(keyText)) == 1
}
func validPassword(password string) bool { return len(password) >= 12 && len(password) <= 1024 }
func emailAddress(text string) (string, error) {
	text = strings.ToLower(strings.TrimSpace(text))
	parsed, err := mail.ParseAddress(text)
	if err != nil || parsed.Address != text || len(text) > 254 {
		return "", bad("请输入有效邮箱")
	}
	return text, nil
}
func (s *Server) register(w http.ResponseWriter, r *http.Request) error {
	if !s.cfg.Registration {
		return denied()
	}
	if !s.allow(r) {
		return fault{429, "rate_limited", "请求过于频繁，请稍后重试"}
	}
	var v struct {
		Email    string `json:"email"`
		Name     string `json:"name"`
		Password string `json:"password"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	email, err := emailAddress(v.Email)
	if err != nil {
		return err
	}
	if !validPassword(v.Password) {
		return bad("密码需要 12–1024 字节")
	}
	if len(v.Name) < 1 || len(v.Name) > 100 {
		return bad("显示名称需要 1–100 字节")
	}
	p, err := passwordHash(v.Password)
	if err != nil {
		return err
	}
	// Public registration never grants administrator privileges, including the first user.
	uid := id()
	_, err = s.db.ExecContext(r.Context(), "INSERT INTO users(id,email,name,password,created,verified) VALUES(?,?,?,?,?,?)", uid, email, v.Name, p, now(), s.cfg.Mailer == nil)
	if err != nil {
		return fault{409, "account_exists", "无法注册此账号"}
	}
	if s.cfg.Mailer != nil {
		if err = s.accountMail(uid, email, "verify"); err != nil {
			return fault{503, "mail_failed", "账号已创建，验证邮件暂未发送；请稍后重新发送验证邮件"}
		}
	}
	write(w, 201, map[string]bool{"registered": true, "verification_required": s.cfg.Mailer != nil})
	return nil
}
func totp(secret []byte, step int64) string {
	var counter [8]byte
	binary.BigEndian.PutUint64(counter[:], uint64(step))
	mac := hmac.New(sha1.New, secret)
	mac.Write(counter[:])
	sum := mac.Sum(nil)
	offset := sum[len(sum)-1] & 15
	value := binary.BigEndian.Uint32(sum[offset:offset+4]) & 0x7fffffff
	return fmt.Sprintf("%06d", value%1000000)
}
func validTOTP(secret []byte, code string, last int64) (int64, bool) {
	step := time.Now().Unix() / 30
	for _, at := range []int64{step, step - 1, step + 1} {
		if at > last && hmac.Equal([]byte(totp(secret, at)), []byte(code)) {
			return at, true
		}
	}
	return 0, false
}
func (s *Server) factor(tx *sql.Tx, uid string, secret []byte, code string, last int64) error {
	if len(secret) == 0 {
		return nil
	}
	plain, err := s.unseal(secret)
	if err != nil {
		return err
	}
	if step, ok := validTOTP(plain, code, last); ok {
		_, err = tx.Exec("UPDATE users SET mfa_last=? WHERE id=?", step, uid)
		return err
	}
	result, err := tx.Exec("DELETE FROM recovery WHERE user_id=? AND hash=?", uid, hash(code))
	if err != nil {
		return err
	}
	n, _ := result.RowsAffected()
	if n != 1 {
		return fault{401, "mfa_required", "请输入有效的一次性验证码或恢复码"}
	}
	return nil
}
func (s *Server) login(w http.ResponseWriter, r *http.Request) error {
	if !s.allow(r) {
		return fault{429, "rate_limited", "登录尝试过多，请稍后重试"}
	}
	var v struct {
		Email    string `json:"email"`
		Password string `json:"password"`
		Code     string `json:"code"`
		Device   string `json:"device"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if len(v.Device) > 200 || len(v.Password) > 1024 {
		return bad("登录字段过长")
	}
	var u user
	var password string
	var secret []byte
	var last int64
	err := s.db.QueryRowContext(r.Context(), "SELECT id,email,name,admin,password,mfa,mfa_last FROM users WHERE email=? AND disabled=0 AND verified=1", strings.ToLower(strings.TrimSpace(v.Email))).Scan(&u.ID, &u.Email, &u.Name, &u.Admin, &password, &secret, &last)
	if err != nil || !passwordOK(password, v.Password) {
		return fault{401, "invalid_credentials", "账号或密码不正确"}
	}
	u.MFA = len(secret) > 0
	token := id() + id()
	err = s.transaction(r.Context(), func(tx *sql.Tx) error {
		// Re-read factor state under the write lock to reject TOTP replay across concurrent logins.
		if err := tx.QueryRow("SELECT mfa,mfa_last FROM users WHERE id=?", u.ID).Scan(&secret, &last); err != nil {
			return err
		}
		if err := s.factor(tx, u.ID, secret, v.Code, last); err != nil {
			return err
		}
		if _, err := tx.Exec("DELETE FROM sessions WHERE expires<=?", now()); err != nil {
			return err
		}
		_, err := tx.Exec("INSERT INTO sessions VALUES(?,?,?,?,?)", hash(token), u.ID, v.Device, now(), now()+int64(s.cfg.SessionTTL.Seconds()))
		return err
	})
	if err != nil {
		return err
	}
	write(w, 200, map[string]any{"token": token, "user": u, "expires": now() + int64(s.cfg.SessionTTL.Seconds())})
	return nil
}
func (s *Server) logout(w http.ResponseWriter, r *http.Request, u user) error {
	_, err := s.db.ExecContext(r.Context(), "DELETE FROM sessions WHERE hash=? AND user_id=?", hash(strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")), u.ID)
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
		s.notify()
	}
	return err
}
func (s *Server) account(w http.ResponseWriter, r *http.Request, u user) error {
	var count int
	var bytes int64
	err := s.db.QueryRowContext(r.Context(), "SELECT COUNT(*),COALESCE(SUM(o.bytes),0) FROM objects o JOIN projects p ON p.id=o.project_id WHERE p.owner=? AND o.deleted=0", u.ID).Scan(&count, &bytes)
	if err != nil {
		return err
	}
	write(w, 200, map[string]any{"user": u, "objects": count, "bytes": bytes, "max_objects": s.cfg.MaxObjects, "max_bytes": s.cfg.MaxBytes})
	return nil
}
func (s *Server) updateAccount(w http.ResponseWriter, r *http.Request, u user) error {
	var v struct {
		Name string `json:"name"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if len(v.Name) == 0 || len(v.Name) > 100 {
		return bad("显示名称需要 1–100 字节")
	}
	_, err := s.db.ExecContext(r.Context(), "UPDATE users SET name=? WHERE id=?", v.Name, u.ID)
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
func (s *Server) changePassword(w http.ResponseWriter, r *http.Request, u user) error {
	if !s.allow(r) {
		return fault{429, "rate_limited", "请稍后重试"}
	}
	var v struct {
		Password    string `json:"password"`
		NewPassword string `json:"new_password"`
		Code        string `json:"code"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if !validPassword(v.NewPassword) {
		return bad("新密码需要 12–1024 字节")
	}
	newHash, err := passwordHash(v.NewPassword)
	if err != nil {
		return err
	}
	err = s.transaction(r.Context(), func(tx *sql.Tx) error {
		var old string
		var mfa []byte
		var last int64
		if err := tx.QueryRow("SELECT password,mfa,mfa_last FROM users WHERE id=?", u.ID).Scan(&old, &mfa, &last); err != nil {
			return err
		}
		if !passwordOK(old, v.Password) {
			return denied()
		}
		if err := s.factor(tx, u.ID, mfa, v.Code, last); err != nil {
			return err
		}
		if _, err := tx.Exec("UPDATE users SET password=? WHERE id=?", newHash, u.ID); err != nil {
			return err
		}
		_, err := tx.Exec("DELETE FROM sessions WHERE user_id=?", u.ID)
		return err
	})
	if err == nil {
		write(w, 200, map[string]bool{"reauthenticate": true})
	}
	return err
}
func (s *Server) mfa(w http.ResponseWriter, r *http.Request, u user) error {
	if !s.allow(r) {
		return fault{429, "rate_limited", "请稍后重试"}
	}
	var v struct {
		Action   string `json:"action"`
		Password string `json:"password"`
		Secret   string `json:"secret"`
		Code     string `json:"code"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	var recovery []string
	if v.Action == "setup" {
		var old string
		if err := s.db.QueryRowContext(r.Context(), "SELECT password FROM users WHERE id=?", u.ID).Scan(&old); err != nil {
			return err
		}
		if !passwordOK(old, v.Password) {
			return denied()
		}
		if u.MFA {
			return bad("两步验证已经启用")
		}
		b := make([]byte, 20)
		if _, err := rand.Read(b); err != nil {
			return err
		}
		write(w, 200, map[string]any{"secret": base32.StdEncoding.WithPadding(base32.NoPadding).EncodeToString(b)})
		return nil
	}
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		var old string
		var secret []byte
		var last int64
		if err := tx.QueryRow("SELECT password,mfa,mfa_last FROM users WHERE id=?", u.ID).Scan(&old, &secret, &last); err != nil {
			return err
		}
		if !passwordOK(old, v.Password) {
			return denied()
		}
		if v.Action == "enable" && len(secret) == 0 {
			b, err := base32.StdEncoding.WithPadding(base32.NoPadding).DecodeString(v.Secret)
			if err != nil || len(b) != 20 {
				return bad("无效的验证密钥")
			}
			step, ok := validTOTP(b, v.Code, 0)
			if !ok {
				return bad("验证码不正确")
			}
			encrypted, err := s.seal(b)
			if err != nil {
				return err
			}
			if _, err = tx.Exec("UPDATE users SET mfa=?,mfa_last=? WHERE id=?", encrypted, step, u.ID); err != nil {
				return err
			}
			for range 10 {
				code := id()
				recovery = append(recovery, code)
				if _, err = tx.Exec("INSERT INTO recovery VALUES(?,?)", u.ID, hash(code)); err != nil {
					return err
				}
			}
		} else if v.Action == "disable" && len(secret) > 0 {
			if err := s.factor(tx, u.ID, secret, v.Code, last); err != nil {
				return err
			}
			if _, err := tx.Exec("UPDATE users SET mfa=NULL,mfa_last=0 WHERE id=?", u.ID); err != nil {
				return err
			}
			if _, err := tx.Exec("DELETE FROM recovery WHERE user_id=?", u.ID); err != nil {
				return err
			}
		} else {
			return bad("两步验证操作不适用")
		}
		_, err := tx.Exec("DELETE FROM sessions WHERE user_id=? AND hash<>?", u.ID, hash(strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")))
		return err
	})
	if err == nil {
		write(w, 200, map[string]any{"ok": true, "recovery_codes": recovery})
	}
	return err
}
func (s *Server) sessions(w http.ResponseWriter, r *http.Request, u user) error {
	rows, err := s.db.QueryContext(r.Context(), "SELECT hash,device,created,expires FROM sessions WHERE user_id=? AND expires>? ORDER BY created DESC LIMIT 100", u.ID, now())
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []map[string]any{}
	for rows.Next() {
		var h, d string
		var c, e int64
		if err = rows.Scan(&h, &d, &c, &e); err != nil {
			return err
		}
		items = append(items, map[string]any{"id": h, "device": d, "created": c, "expires": e})
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}
func (s *Server) revokeSession(w http.ResponseWriter, r *http.Request, u user) error {
	_, err := s.db.ExecContext(r.Context(), "DELETE FROM sessions WHERE user_id=? AND hash=?", u.ID, r.PathValue("hash"))
	if err == nil {
		s.notify()
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}

// CreateAdmin is a local bootstrap command; it never embeds credentials in an executable.
func (s *Server) CreateAdmin(email, name, password string) error {
	email, err := emailAddress(email)
	if err != nil {
		return err
	}
	if !validPassword(password) {
		return bad("password must contain at least 12 bytes")
	}
	p, err := passwordHash(password)
	if err != nil {
		return err
	}
	_, err = s.db.Exec("INSERT INTO users(id,email,name,password,admin,created,verified) VALUES(?,?,?,?,1,?,1)", id(), email, name, p, now())
	return err
}
func (s *Server) ResetPassword(email, password string) error {
	if !validPassword(password) {
		return bad("password must contain at least 12 bytes")
	}
	p, err := passwordHash(password)
	if err != nil {
		return err
	}
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	result, err := tx.Exec("UPDATE users SET password=?,mfa=NULL,mfa_last=0 WHERE email=?", p, strings.ToLower(email))
	if err != nil {
		return err
	}
	n, _ := result.RowsAffected()
	if n != 1 {
		return missing()
	}
	if _, err = tx.Exec("DELETE FROM sessions WHERE user_id=(SELECT id FROM users WHERE email=?)", strings.ToLower(email)); err != nil {
		return err
	}
	if _, err = tx.Exec("DELETE FROM recovery WHERE user_id=(SELECT id FROM users WHERE email=?)", strings.ToLower(email)); err != nil {
		return err
	}
	return tx.Commit()
}
func (s *Server) adminUsers(w http.ResponseWriter, r *http.Request, u user) error {
	if !u.Admin {
		return denied()
	}
	rows, err := s.db.QueryContext(r.Context(), "SELECT id,email,name,admin,disabled FROM users WHERE id>? ORDER BY id LIMIT ?", r.URL.Query().Get("after"), limit(r))
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []map[string]any{}
	for rows.Next() {
		var id, e, n string
		var a, d bool
		if err = rows.Scan(&id, &e, &n, &a, &d); err != nil {
			return err
		}
		items = append(items, map[string]any{"id": id, "email": e, "name": n, "admin": a, "disabled": d})
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}
func (s *Server) adminUser(w http.ResponseWriter, r *http.Request, u user) error {
	if !u.Admin || r.PathValue("user") == u.ID {
		return denied()
	}
	var v struct {
		Disabled bool `json:"disabled"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		if _, err := tx.Exec("UPDATE users SET disabled=? WHERE id=?", v.Disabled, r.PathValue("user")); err != nil {
			return err
		}
		if v.Disabled {
			_, err := tx.Exec("DELETE FROM sessions WHERE user_id=?", r.PathValue("user"))
			return err
		}
		return nil
	})
	if err == nil {
		write(w, 200, map[string]bool{"ok": true})
	}
	return err
}
