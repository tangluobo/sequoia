package collaboration

import (
	"context"
	"crypto/tls"
	"database/sql"
	"fmt"
	"net"
	"net/http"
	"net/mail"
	"net/smtp"
	"net/url"
	"strings"
	"time"
)

type Mailer interface {
	Send(to, subject, body string) error
}
type SMTP struct{ Address, Username, Password, From string }

func (m SMTP) Send(to, subject, body string) error {
	host, _, err := net.SplitHostPort(m.Address)
	if err != nil {
		return err
	}
	if _, err = mail.ParseAddress(m.From); err != nil {
		return err
	}
	connection, err := net.DialTimeout("tcp", m.Address, 5*time.Second)
	if err != nil {
		return err
	}
	defer connection.Close()
	if err = connection.SetDeadline(time.Now().Add(10 * time.Second)); err != nil {
		return err
	}
	client, err := smtp.NewClient(connection, host)
	if err != nil {
		return err
	}
	defer client.Close()
	if ok, _ := client.Extension("STARTTLS"); !ok {
		return fmt.Errorf("SMTP server must support STARTTLS")
	}
	if err = client.StartTLS(&tls.Config{ServerName: host, MinVersion: tls.VersionTLS12}); err != nil {
		return err
	}
	if m.Username != "" {
		if err = client.Auth(smtp.PlainAuth("", m.Username, m.Password, host)); err != nil {
			return err
		}
	}
	if err = client.Mail(m.From); err != nil {
		return err
	}
	if err = client.Rcpt(to); err != nil {
		return err
	}
	writer, err := client.Data()
	if err != nil {
		return err
	}
	// to/from are parsed addresses, subject is a static string, not a user field.
	_, err = fmt.Fprintf(writer, "From: %s\r\nTo: %s\r\nSubject: %s\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n%s\r\n", m.From, to, subject, body)
	if err != nil {
		writer.Close()
		return err
	}
	if err = writer.Close(); err != nil {
		return err
	}
	return client.Quit()
}
func (s *Server) accountMail(uid, email, purpose string) error {
	if s.cfg.Mailer == nil {
		return fault{503, "mail_unavailable", "管理员尚未配置邮件服务"}
	}
	base, err := url.Parse(s.cfg.PublicURL)
	if err != nil || base.Host == "" || base.RawQuery != "" || base.Fragment != "" || base.User != nil {
		return fmt.Errorf("invalid public URL")
	}
	token := id() + id()
	expiry := now() + 3600
	if purpose == "reset" {
		expiry = now() + 900
	}
	err = s.transaction(context.Background(), func(tx *sql.Tx) error {
		if _, err := tx.Exec("DELETE FROM account_tokens WHERE expires<=? OR (user_id=? AND purpose=?)", now(), uid, purpose); err != nil {
			return err
		}
		_, err := tx.Exec("INSERT INTO account_tokens VALUES(?,?,?,?,?)", hash(token), uid, purpose, expiry, now())
		return err
	})
	if err != nil {
		return err
	}
	query := base.Query()
	query.Set(purpose, token)
	base.RawQuery = query.Encode()
	return s.cfg.Mailer.Send(email, "Tomato Cloud account verification", "Open this link to "+purpose+" your Tomato Cloud account.\n\n"+base.String()+"\n\nIf you did not request this, ignore this message.")
}
func (s *Server) requestAccountMail(w http.ResponseWriter, r *http.Request) error {
	if !s.allow(r) {
		return fault{429, "rate_limited", "请求过于频繁"}
	}
	var v struct {
		Email   string `json:"email"`
		Purpose string `json:"purpose"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if v.Purpose != "verify" && v.Purpose != "reset" {
		return bad("账号操作无效")
	}
	if s.cfg.Mailer == nil {
		return fault{503, "mail_unavailable", "请联系服务器管理员恢复账号"}
	}
	var uid, email string
	var verified bool
	err := s.db.QueryRowContext(r.Context(), "SELECT id,email,verified FROM users WHERE email=? AND disabled=0", strings.ToLower(strings.TrimSpace(v.Email))).Scan(&uid, &email, &verified)
	if err == nil && (v.Purpose == "reset" || !verified) {
		_ = s.accountMail(uid, email, v.Purpose)
	}
	write(w, 200, map[string]bool{"requested": true})
	return nil
}
func (s *Server) verifyAccount(w http.ResponseWriter, r *http.Request) error {
	var v struct {
		Token string `json:"token"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	err := s.transaction(r.Context(), func(tx *sql.Tx) error {
		var uid string
		if tx.QueryRow("SELECT user_id FROM account_tokens WHERE hash=? AND purpose='verify' AND expires>?", hash(v.Token), now()).Scan(&uid) != nil {
			return bad("验证链接无效或已过期")
		}
		if _, err := tx.Exec("UPDATE users SET verified=1 WHERE id=?", uid); err != nil {
			return err
		}
		_, err := tx.Exec("DELETE FROM account_tokens WHERE user_id=? AND purpose='verify'", uid)
		return err
	})
	if err == nil {
		write(w, 200, map[string]bool{"verified": true})
	}
	return err
}
func (s *Server) resetAccount(w http.ResponseWriter, r *http.Request) error {
	if !s.allow(r) {
		return fault{429, "rate_limited", "请求过于频繁"}
	}
	var v struct {
		Token    string `json:"token"`
		Password string `json:"password"`
	}
	if err := decode(r, &v); err != nil {
		return err
	}
	if !validPassword(v.Password) {
		return bad("密码需要 12–1024 字节")
	}
	p, err := passwordHash(v.Password)
	if err != nil {
		return err
	}
	err = s.transaction(r.Context(), func(tx *sql.Tx) error {
		var uid string
		if tx.QueryRow("SELECT user_id FROM account_tokens WHERE hash=? AND purpose='reset' AND expires>?", hash(v.Token), now()).Scan(&uid) != nil {
			return bad("密码恢复链接无效或已过期")
		}
		if _, err := tx.Exec("UPDATE users SET password=? WHERE id=?", p, uid); err != nil {
			return err
		}
		if _, err := tx.Exec("DELETE FROM sessions WHERE user_id=?", uid); err != nil {
			return err
		}
		_, err := tx.Exec("DELETE FROM account_tokens WHERE user_id=?", uid)
		return err
	})
	if err == nil {
		write(w, 200, map[string]bool{"reset": true})
	}
	return err
}
func (s *Server) createUser(w http.ResponseWriter, r *http.Request, u user) error {
	if !u.Admin {
		return denied()
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
	if !validPassword(v.Password) || !validName(v.Name) {
		return bad("名称或密码无效")
	}
	p, err := passwordHash(v.Password)
	if err != nil {
		return err
	}
	_, err = s.db.ExecContext(r.Context(), "INSERT INTO users(id,email,name,password,verified,created) VALUES(?,?,?,?,1,?)", id(), email, v.Name, p, now())
	if err == nil {
		write(w, 201, map[string]bool{"created": true})
	}
	return err
}
