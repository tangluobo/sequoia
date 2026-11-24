package collaboration

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"strconv"
	"strings"
	"time"
)

type fault struct {
	Status  int
	Code    string
	Message string
}

func (e fault) Error() string  { return e.Message }
func bad(message string) error { return fault{400, "invalid", message} }
func denied() error            { return fault{403, "forbidden", "没有此操作的权限"} }
func missing() error           { return fault{404, "not_found", "项目或对象不存在"} }
func conflict() error {
	return fault{409, "conflict", "远端版本已改变，保留本地内容后重新合并"}
}

type handler func(http.ResponseWriter, *http.Request) error

func (s *Server) route(mux *http.ServeMux, pattern string, fn handler) {
	mux.HandleFunc(pattern, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")
		// Browser requests may only come from this origin; desktop clients don't send Origin.
		if origin := r.Header.Get("Origin"); origin != "" && origin != "https://"+r.Host && origin != "http://"+r.Host {
			respondError(w, denied())
			return
		}
		r.Body = http.MaxBytesReader(w, r.Body, MaxObject+16384)
		if err := fn(w, r); err != nil {
			respondError(w, err)
		}
	})
}
func respondError(w http.ResponseWriter, err error) {
	status := 500
	code := "internal"
	message := "服务端操作失败"
	var f fault
	if errors.As(err, &f) {
		status = f.Status
		code = f.Code
		message = f.Message
	}
	write(w, status, map[string]any{"error": code, "message": message})
}
func write(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}
func decode(r *http.Request, out any) error {
	d := json.NewDecoder(r.Body)
	d.DisallowUnknownFields()
	if err := d.Decode(out); err != nil {
		return bad("请求格式无效或超过大小限制")
	}
	var extra any
	if errors.Is(d.Decode(&extra), io.EOF) {
		return nil
	}
	return bad("只允许一个 JSON 文档")
}
func hash(text string) string { sum := sha256.Sum256([]byte(text)); return hex.EncodeToString(sum[:]) }

type user struct {
	ID    string `json:"id"`
	Email string `json:"email"`
	Name  string `json:"name"`
	Admin bool   `json:"admin"`
	MFA   bool   `json:"mfa"`
}

func (s *Server) authenticate(r *http.Request) (user, error) {
	token := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
	var u user
	if len(token) != 64 {
		return u, fault{401, "unauthorized", "请登录云端账号"}
	}
	err := s.db.QueryRowContext(r.Context(), "SELECT u.id,u.email,u.name,u.admin,u.mfa IS NOT NULL FROM users u JOIN sessions t ON t.user_id=u.id WHERE t.hash=? AND t.expires>? AND u.disabled=0", hash(token), now()).Scan(&u.ID, &u.Email, &u.Name, &u.Admin, &u.MFA)
	if err != nil {
		return u, fault{401, "unauthorized", "登录已失效，请重新登录"}
	}
	return u, nil
}
func (s *Server) authenticated(fn func(http.ResponseWriter, *http.Request, user) error) handler {
	return func(w http.ResponseWriter, r *http.Request) error {
		u, err := s.authenticate(r)
		if err != nil {
			return err
		}
		return fn(w, r, u)
	}
}
func (s *Server) allow(r *http.Request) bool {
	ip, _, _ := net.SplitHostPort(r.RemoteAddr)
	s.limits.Lock()
	defer s.limits.Unlock()
	t := time.Now()
	a := s.attempts[ip]
	if a.Until.Before(t) {
		a = attempt{Until: t.Add(time.Minute)}
	}
	if len(s.attempts) > 10000 {
		for k, v := range s.attempts {
			if v.Until.Before(t) {
				delete(s.attempts, k)
			}
		}
		if len(s.attempts) > 10000 {
			return false
		}
	}
	a.Count++
	s.attempts[ip] = a
	return a.Count <= 12
}
func limit(r *http.Request) int {
	n, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	if n < 1 || n > 200 {
		return 100
	}
	return n
}
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	s.route(mux, "GET /healthz", func(w http.ResponseWriter, r *http.Request) error {
		ctx, cancel := context.WithTimeout(r.Context(), time.Second)
		defer cancel()
		if err := s.db.PingContext(ctx); err != nil {
			return err
		}
		write(w, 200, map[string]any{"status": "ok", "protocol": 1})
		return nil
	})
	s.route(mux, "POST /v1/register", s.register)
	s.route(mux, "POST /v1/login", s.login)
	s.route(mux, "POST /v1/account-mail", s.requestAccountMail)
	s.route(mux, "POST /v1/verify", s.verifyAccount)
	s.route(mux, "POST /v1/reset", s.resetAccount)
	s.route(mux, "POST /v1/logout", s.authenticated(s.logout))
	s.route(mux, "GET /v1/account", s.authenticated(s.account))
	s.route(mux, "GET /v1/profile", s.authenticated(s.getProfile))
	s.route(mux, "PUT /v1/profile", s.authenticated(s.putProfile))
	s.route(mux, "PUT /v1/account", s.authenticated(s.updateAccount))
	s.route(mux, "POST /v1/account/password", s.authenticated(s.changePassword))
	s.route(mux, "POST /v1/account/mfa", s.authenticated(s.mfa))
	s.route(mux, "GET /v1/sessions", s.authenticated(s.sessions))
	s.route(mux, "DELETE /v1/sessions/{hash}", s.authenticated(s.revokeSession))
	s.route(mux, "GET /v1/projects", s.authenticated(s.projects))
	s.route(mux, "POST /v1/projects", s.authenticated(s.createProject))
	s.route(mux, "PATCH /v1/projects/{project}", s.authenticated(s.renameProject))
	s.route(mux, "DELETE /v1/projects/{project}", s.authenticated(s.deleteProject))
	s.route(mux, "GET /v1/projects/{project}/members", s.authenticated(s.members))
	s.route(mux, "PUT /v1/projects/{project}/members/{member}", s.authenticated(s.setMember))
	s.route(mux, "DELETE /v1/projects/{project}/members/{member}", s.authenticated(s.removeMember))
	s.route(mux, "POST /v1/projects/{project}/invitations", s.authenticated(s.invite))
	s.route(mux, "GET /v1/invitations", s.authenticated(s.invitations))
	s.route(mux, "POST /v1/invitations/{invitation}", s.authenticated(s.accept))
	s.route(mux, "DELETE /v1/projects/{project}/invitations/{invitation}", s.authenticated(s.cancelInvitation))
	s.route(mux, "GET /v1/projects/{project}/objects", s.authenticated(s.objects))
	s.route(mux, "GET /v1/projects/{project}/objects/{object}", s.authenticated(s.getObject))
	s.route(mux, "PUT /v1/projects/{project}/objects/{object}", s.authenticated(s.putObject))
	s.route(mux, "DELETE /v1/projects/{project}/objects/{object}", s.authenticated(s.deleteObject))
	s.route(mux, "GET /v1/projects/{project}/objects/{object}/history", s.authenticated(s.history))
	s.route(mux, "POST /v1/projects/{project}/transfer", s.authenticated(s.transfer))
	s.route(mux, "GET /v1/events", s.authenticated(s.events))
	s.route(mux, "GET /v1/projects/{project}/activity", s.authenticated(s.activity))
	s.route(mux, "GET /v1/admin/users", s.authenticated(s.adminUsers))
	s.route(mux, "POST /v1/admin/users", s.authenticated(s.createUser))
	s.route(mux, "PUT /v1/admin/users/{user}", s.authenticated(s.adminUser))
	mux.Handle("/", portal())
	return mux
}
