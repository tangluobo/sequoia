package collaboration

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/base32"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

type fixture struct {
	t    *testing.T
	s    *Server
	http *httptest.Server
}

type mailbox struct {
	mu       sync.Mutex
	messages []string
}

func (m *mailbox) Send(to, subject, body string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.messages = append(m.messages, body)
	return nil
}
func (m *mailbox) token(t *testing.T, purpose string) string {
	t.Helper()
	m.mu.Lock()
	defer m.mu.Unlock()
	for i := len(m.messages) - 1; i >= 0; i-- {
		for _, line := range strings.Split(m.messages[i], "\n") {
			if strings.HasPrefix(line, "https://") {
				u, e := url.Parse(line)
				if e != nil {
					t.Fatal(e)
				}
				if value := u.Query().Get(purpose); value != "" {
					return value
				}
			}
		}
	}
	t.Fatal("message token missing")
	return ""
}
func TestVerifiedRegistrationPasswordRecoveryAndPrivateProfiles(t *testing.T) {
	f := newFixture(t)
	mail := &mailbox{}
	f.s.cfg.Mailer = mail
	f.s.cfg.PublicURL = "https://cloud.example.test"
	f.request("POST", "/v1/register", "", map[string]string{"email": "verified@example.test", "name": "user", "password": "test-strong-pass-123"}, 201)
	f.request("POST", "/v1/login", "", map[string]string{"email": "verified@example.test", "password": "test-strong-pass-123"}, 401)
	verify := mail.token(t, "verify")
	f.request("POST", "/v1/verify", "", map[string]string{"token": verify}, 200)
	f.request("POST", "/v1/verify", "", map[string]string{"token": verify}, 400)
	token := f.login("verified@example.test", "")
	profile := Mutation{Kind: "profile", Name: "个人配置", Body: json.RawMessage(`{"appearance":{"sshTerminalFontSize":15},"connections":{}}`), Mutation: id()}
	f.request("PUT", "/v1/profile", token, profile, 200)
	f.s.cfg.Mailer = nil
	other := f.user("other@example.test")
	var private Object
	json.Unmarshal(f.request("GET", "/v1/profile", other, nil, 200), &private)
	if private.Revision != 0 {
		t.Fatal("personal profile leaked to another user")
	}
	f.s.cfg.Mailer = mail
	profile.Mutation = id()
	profile.Body = json.RawMessage(`{"appearance":{"aiApiKey":"never-upload"}}`)
	profile.Base = 1
	f.request("PUT", "/v1/profile", token, profile, 400)
	f.request("POST", "/v1/account-mail", "", map[string]string{"email": "verified@example.test", "purpose": "reset"}, 200)
	reset := mail.token(t, "reset")
	f.request("POST", "/v1/reset", "", map[string]string{"token": reset, "password": "new-strong-password-123"}, 200)
	f.request("GET", "/v1/account", token, nil, 401)
	f.request("POST", "/v1/reset", "", map[string]string{"token": reset, "password": "new-strong-password-123"}, 400)
	f.request("POST", "/v1/login", "", map[string]string{"email": "verified@example.test", "password": "new-strong-password-123"}, 200)
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	s, e := Open(Config{DataDir: t.TempDir(), Registration: true})
	if e != nil {
		t.Fatal(e)
	}
	http := httptest.NewServer(s.Handler())
	t.Cleanup(func() { http.Close(); s.Close() })
	return &fixture{t, s, http}
}
func (f *fixture) request(method, path, token string, body any, status int) json.RawMessage {
	f.t.Helper()
	b, _ := json.Marshal(body)
	req, e := http.NewRequest(method, f.http.URL+path, bytes.NewReader(b))
	if e != nil {
		f.t.Fatal(e)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")
	resp, e := http.DefaultClient.Do(req)
	if e != nil {
		f.t.Fatal(e)
	}
	defer resp.Body.Close()
	out, e := io.ReadAll(resp.Body)
	if e != nil {
		f.t.Fatal(e)
	}
	if resp.StatusCode != status {
		f.t.Fatalf("%s %s: got %d want %d: %s", method, path, resp.StatusCode, status, out)
	}
	return out
}
func (f *fixture) user(email string) string {
	f.request("POST", "/v1/register", "", map[string]string{"email": email, "name": email, "password": "test-strong-pass-123"}, 201)
	return f.login(email, "")
}
func (f *fixture) login(email, code string) string {
	out := f.request("POST", "/v1/login", "", map[string]string{"email": email, "password": "test-strong-pass-123", "code": code, "device": "fixture"}, 200)
	var v struct{ Token string }
	json.Unmarshal(out, &v)
	return v.Token
}
func (f *fixture) project(token, name string) project {
	var p project
	json.Unmarshal(f.request("POST", "/v1/projects", token, map[string]string{"name": name}, 201), &p)
	return p
}
func (f *fixture) object(token, pid, oid, parent, name string, revision int64, status int) Object {
	var o Object
	json.Unmarshal(f.request("PUT", "/v1/projects/"+pid+"/objects/"+oid, token, Mutation{Parent: parent, Kind: "query", Name: name, Body: json.RawMessage(`{"text":"select '中文🙂'"}`), Base: revision, Mutation: id()}, status), &o)
	return o
}
func addMember(t *testing.T, s *Server, pid, email, role string) {
	t.Helper()
	if _, e := s.db.Exec("INSERT INTO members SELECT ?,id,? FROM users WHERE email=?", pid, role, email); e != nil {
		t.Fatal(e)
	}
}

func TestProjectsPermissionsInvitationsAndRevocation(t *testing.T) {
	f := newFixture(t)
	owner := f.user("owner@example.test")
	editor := f.user("editor@example.test")
	viewer := f.user("viewer@example.test")
	outsider := f.user("outsider@example.test")
	p := f.project(owner, "项目🙂")
	f.request("GET", "/v1/projects/"+p.ID+"/objects", outsider, nil, 404)
	f.request("POST", "/v1/projects/"+p.ID+"/invitations", owner, map[string]string{"email": "editor@example.test", "role": "editor"}, 201)
	var invitations []map[string]any
	json.Unmarshal(f.request("GET", "/v1/invitations", editor, nil, 200), &invitations)
	if len(invitations) != 1 {
		t.Fatal(invitations)
	}
	f.request("POST", "/v1/invitations/"+invitations[0]["id"].(string), outsider, map[string]bool{"accept": true}, 403)
	f.request("POST", "/v1/invitations/"+invitations[0]["id"].(string), editor, map[string]bool{"accept": true}, 200)
	addMember(t, f.s, p.ID, "viewer@example.test", "viewer")
	o := f.object(editor, p.ID, "one", "", "查询", 0, 200)
	f.object(viewer, p.ID, "one", "", "forbidden", o.Revision, 403)
	f.request("DELETE", "/v1/projects/"+p.ID, editor, nil, 403)
	f.request("PATCH", "/v1/projects/"+p.ID, editor, map[string]any{"name": "x", "revision": 1}, 403)
	f.request("DELETE", "/v1/projects/"+p.ID, owner, nil, 400)
	var members []map[string]any
	json.Unmarshal(f.request("GET", "/v1/projects/"+p.ID+"/members", owner, nil, 200), &members)
	var eid, ownerID string
	for _, m := range members {
		if m["email"] == "editor@example.test" {
			eid = m["id"].(string)
		}
		if m["role"] == "owner" {
			ownerID = m["id"].(string)
		}
	}
	f.request("PUT", "/v1/projects/"+p.ID+"/members/"+ownerID, owner, map[string]string{"role": "viewer"}, 403)
	f.request("DELETE", "/v1/projects/"+p.ID+"/members/"+eid, owner, nil, 200)
	f.request("GET", "/v1/projects/"+p.ID+"/objects/one", editor, nil, 404)
	var e struct{ Events []Event }
	json.Unmarshal(f.request("GET", "/v1/events?cursor=0&wait=0", editor, nil, 200), &e)
	if len(e.Events) == 0 || e.Events[len(e.Events)-1].Action != "member_removed" {
		t.Fatalf("revocation is not observable: %+v", e)
	}
}
func TestOptimisticWritesIdempotencyHistoryAndSecrets(t *testing.T) {
	f := newFixture(t)
	token := f.user("owner@example.test")
	p := f.project(token, "workspace")
	path := "/v1/projects/" + p.ID + "/objects/one"
	m := Mutation{Kind: "query", Name: "first", Body: json.RawMessage(`{"text":"select 1"}`), Mutation: id()}
	f.request("PUT", path, token, m, 200)
	f.request("PUT", path, token, m, 200)
	var versions []map[string]any
	json.Unmarshal(f.request("GET", path+"/history", token, nil, 200), &versions)
	if len(versions) != 1 {
		t.Fatalf("idempotent retry created %d versions", len(versions))
	}
	f.object(token, p.ID, "one", "", "stale", 0, 409)
	o := f.object(token, p.ID, "one", "", "new", 1, 200)
	if o.Revision != 2 {
		t.Fatal(o)
	}
	f.request("GET", path+"?revision=1", token, nil, 200)
	m.Kind = "connection"
	m.Name = "secret"
	m.Mutation = id()
	m.Body = json.RawMessage(`{"type":"MYSQL","password":"never-cloud"}`)
	f.request("PUT", "/v1/projects/"+p.ID+"/objects/secret", token, m, 400)
	m.Body = json.RawMessage(`{"type":"MYSQL","host":"mysql://u:p@host"}`)
	f.request("PUT", "/v1/projects/"+p.ID+"/objects/secret", token, m, 400)
	m.Body = json.RawMessage(`{"type":"MYSQL","host":"localhost","username":"u"}`)
	f.request("PUT", "/v1/projects/"+p.ID+"/objects/secret", token, m, 200)
	var encrypted []byte
	if e := f.s.db.QueryRow("SELECT body FROM objects WHERE id='one'").Scan(&encrypted); e != nil {
		t.Fatal(e)
	}
	if bytes.Contains(encrypted, []byte("中文")) {
		t.Fatal("object stored as plaintext")
	}
	plain, e := f.s.unseal(encrypted)
	if e != nil || !bytes.Contains(plain, []byte("中文")) {
		t.Fatal(e, string(plain))
	}
	f.request("POST", "/v1/logout", token, map[string]any{}, 200)
	f.request("GET", path, token, nil, 401)
}
func TestConcurrentUpdatesHaveOneWinner(t *testing.T) {
	f := newFixture(t)
	token := f.user("owner@example.test")
	p := f.project(token, "concurrent")
	f.object(token, p.ID, "one", "", "base", 0, 200)
	var wg sync.WaitGroup
	results := make(chan int, 2)
	for i := 0; i < 2; i++ {
		wg.Go(func() {
			m := Mutation{Kind: "query", Name: "edit", Body: json.RawMessage(`{"text":"one"}`), Base: 1, Mutation: id()}
			b, _ := json.Marshal(m)
			req, _ := http.NewRequest("PUT", f.http.URL+"/v1/projects/"+p.ID+"/objects/one", bytes.NewReader(b))
			req.Header.Set("Authorization", "Bearer "+token)
			resp, e := http.DefaultClient.Do(req)
			if e != nil {
				results <- 0
				return
			}
			io.Copy(io.Discard, resp.Body)
			resp.Body.Close()
			results <- resp.StatusCode
		})
	}
	wg.Wait()
	close(results)
	n := map[int]int{}
	for r := range results {
		n[r]++
	}
	if n[200] != 1 || n[409] != 1 {
		t.Fatal(n)
	}
}
func TestGroupsTransferTombstonesAndQuota(t *testing.T) {
	f := newFixture(t)
	token := f.user("owner@example.test")
	a := f.project(token, "a")
	b := f.project(token, "b")
	f.request("PUT", "/v1/projects/"+a.ID+"/objects/connection", token, Mutation{Kind: "connection", Name: "mysql", Body: json.RawMessage(`{"type":"MYSQL","host":"localhost"}`), Mutation: id()}, 200)
	f.object(token, a.ID, "child", "connection", "q", 0, 200)
	f.request("PUT", "/v1/projects/"+a.ID+"/objects/connection", token, Mutation{Parent: "child", Kind: "connection", Name: "cycle", Body: json.RawMessage(`{"type":"MYSQL"}`), Base: 1, Mutation: id()}, 400)
	out := f.request("POST", "/v1/projects/"+a.ID+"/transfer", token, map[string]any{"objects": []string{"connection"}, "target": b.ID, "copy": false}, 200)
	var moved []Object
	json.Unmarshal(out, &moved)
	if len(moved) != 2 {
		t.Fatal(string(out))
	}
	for _, o := range moved {
		if o.Name == "q" && o.Parent == "" {
			t.Fatal("lost connection relationship")
		}
	}
	var old Object
	json.Unmarshal(f.request("GET", "/v1/projects/"+a.ID+"/objects/child", token, nil, 200), &old)
	if !old.Deleted || old.Revision != 2 {
		t.Fatal(old)
	}
	f.s.cfg.MaxObjects = 2
	f.object(token, b.ID, "excess", "", "q", 0, 413)
	f.request("DELETE", "/v1/projects/"+a.ID, token, nil, 200)
}
func TestTwoFactorRecoveryAndSessionRevocation(t *testing.T) {
	f := newFixture(t)
	token := f.user("owner@example.test")
	other := f.login("owner@example.test", "")
	var setup struct{ Secret string }
	json.Unmarshal(f.request("POST", "/v1/account/mfa", token, map[string]string{"action": "setup", "password": "test-strong-pass-123"}, 200), &setup)
	secret, e := base32.StdEncoding.WithPadding(base32.NoPadding).DecodeString(setup.Secret)
	if e != nil {
		t.Fatal(e)
	}
	var enabled struct {
		Codes []string `json:"recovery_codes"`
	}
	json.Unmarshal(f.request("POST", "/v1/account/mfa", token, map[string]string{"action": "enable", "password": "test-strong-pass-123", "secret": setup.Secret, "code": totp(secret, now()/30)}, 200), &enabled)
	if len(enabled.Codes) != 10 {
		t.Fatal(enabled)
	}
	f.request("GET", "/v1/account", other, nil, 401)
	f.request("POST", "/v1/login", "", map[string]string{"email": "owner@example.test", "password": "test-strong-pass-123"}, 401)
	_ = f.login("owner@example.test", enabled.Codes[0])
	f.request("POST", "/v1/login", "", map[string]string{"email": "owner@example.test", "password": "test-strong-pass-123", "code": enabled.Codes[0]}, 401)
}
func TestBackupRestartAndKeyMismatch(t *testing.T) {
	dir := t.TempDir()
	cfg := Config{DataDir: filepath.Join(dir, "data"), Registration: true}
	s, e := Open(cfg)
	if e != nil {
		t.Fatal(e)
	}
	if e = s.CreateAdmin("a@example.test", "admin", "test-strong-pass-123"); e != nil {
		t.Fatal(e)
	}
	backup := filepath.Join(dir, "backup")
	if e = s.Backup(backup); e != nil {
		t.Fatal(e)
	}
	s.Close()
	restored, e := Open(Config{DataDir: backup})
	if e != nil {
		t.Fatal(e)
	}
	var n int
	restored.db.QueryRow("SELECT COUNT(*) FROM users").Scan(&n)
	restored.Close()
	if n != 1 {
		t.Fatal(n)
	}
	if e = os.WriteFile(filepath.Join(backup, "encryption.key"), make([]byte, 32), 0600); e != nil {
		t.Fatal(e)
	}
	if bad, e := Open(Config{DataDir: backup}); e == nil {
		bad.Close()
		t.Fatal("accepted wrong key")
	}
}
func TestLongPollCancellationAndAuthorization(t *testing.T) {
	f := newFixture(t)
	token := f.user("owner@example.test")
	ctx, cancel := context.WithCancel(context.Background())
	req, _ := http.NewRequestWithContext(ctx, "GET", f.http.URL+"/v1/events?cursor=9999&wait=25", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	done := make(chan error, 1)
	go func() {
		resp, e := http.DefaultClient.Do(req)
		if resp != nil {
			resp.Body.Close()
		}
		done <- e
	}()
	time.Sleep(20 * time.Millisecond)
	cancel()
	select {
	case e := <-done:
		if e == nil {
			t.Fatal("did not cancel")
		}
	case <-time.After(time.Second):
		t.Fatal("cancellation blocked")
	}
}
func TestPaginationAndRequestValidation(t *testing.T) {
	f := newFixture(t)
	token := f.user("owner@example.test")
	p := f.project(token, "pages")
	e := f.s.transaction(context.Background(), func(tx *sql.Tx) error {
		for i := 0; i < 205; i++ {
			o := Object{ID: fmt.Sprintf("o%04d", i), Project: p.ID, Kind: "query", Name: "n", Body: json.RawMessage(`{"text":""}`), Bytes: 11, Revision: 1, Modified: now(), ModifiedBy: "fixture", Mutation: id()}
			if e := f.s.storeObject(tx, o); e != nil {
				return e
			}
		}
		return nil
	})
	if e != nil {
		t.Fatal(e)
	}
	var first, second []Object
	json.Unmarshal(f.request("GET", "/v1/projects/"+p.ID+"/objects?limit=200", token, nil, 200), &first)
	json.Unmarshal(f.request("GET", "/v1/projects/"+p.ID+"/objects?after="+first[len(first)-1].ID, token, nil, 200), &second)
	if len(first) != 200 || len(second) != 5 {
		t.Fatal(len(first), len(second))
	}
	req, _ := http.NewRequest("POST", f.http.URL+"/v1/projects", strings.NewReader(`{"name":"valid"} garbage`))
	req.Header.Set("Authorization", "Bearer "+token)
	resp, e := http.DefaultClient.Do(req)
	if e != nil {
		t.Fatal(e)
	}
	defer resp.Body.Close()
	if resp.StatusCode != 400 {
		t.Fatal(resp.StatusCode)
	}
}
