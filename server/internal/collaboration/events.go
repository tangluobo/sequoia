package collaboration

import (
	"net/http"
	"strconv"
	"time"
)

type Event struct {
	Seq       int64  `json:"seq"`
	Project   string `json:"project_id"`
	User      string `json:"user_id"`
	Object    string `json:"object_id"`
	Action    string `json:"action"`
	Created   int64  `json:"created"`
	Actor     string `json:"actor_id"`
	ActorName string `json:"actor_name"`
}

func (s *Server) events(w http.ResponseWriter, r *http.Request, u user) error {
	cursor, err := strconv.ParseInt(r.URL.Query().Get("cursor"), 10, 64)
	if err != nil || cursor < 0 {
		return bad("同步游标无效")
	}
	wait, _ := strconv.Atoi(r.URL.Query().Get("wait"))
	if wait < 0 {
		wait = 0
	}
	if wait > 25 {
		wait = 25
	}
	timer := time.NewTimer(time.Duration(wait) * time.Second)
	defer timer.Stop()
	for {
		notify := s.listener()
		if _, err = s.authenticate(r); err != nil {
			return err
		}
		rows, err := s.db.QueryContext(r.Context(), "SELECT seq,project_id,user_id,object_id,action,created,actor_id,COALESCE((SELECT name FROM users WHERE id=events.actor_id),actor_id) FROM events WHERE seq>? AND (project_id IN (SELECT project_id FROM members WHERE user_id=?) OR user_id=?) ORDER BY seq LIMIT 100", cursor, u.ID, u.ID)
		if err != nil {
			return err
		}
		items := []Event{}
		for rows.Next() {
			var e Event
			if err = rows.Scan(&e.Seq, &e.Project, &e.User, &e.Object, &e.Action, &e.Created, &e.Actor, &e.ActorName); err != nil {
				rows.Close()
				return err
			}
			items = append(items, e)
		}
		if err = rows.Err(); err != nil {
			rows.Close()
			return err
		}
		rows.Close()
		if len(items) > 0 {
			cursor = items[len(items)-1].Seq
			write(w, 200, map[string]any{"cursor": cursor, "events": items})
			return nil
		}
		select {
		case <-notify:
			continue
		case <-r.Context().Done():
			return nil
		case <-timer.C:
			write(w, 200, map[string]any{"cursor": cursor, "events": items})
			return nil
		}
	}
}
func (s *Server) activity(w http.ResponseWriter, r *http.Request, u user) error {
	pid := r.PathValue("project")
	if err := require(s.db, pid, u.ID, false, false); err != nil {
		return err
	}
	before, _ := strconv.ParseInt(r.URL.Query().Get("before"), 10, 64)
	if before <= 0 {
		before = 1 << 62
	}
	rows, err := s.db.QueryContext(r.Context(), "SELECT seq,project_id,user_id,object_id,action,created,actor_id,COALESCE((SELECT name FROM users WHERE id=events.actor_id),actor_id) FROM events WHERE project_id=? AND seq<? ORDER BY seq DESC LIMIT ?", pid, before, limit(r))
	if err != nil {
		return err
	}
	defer rows.Close()
	items := []Event{}
	for rows.Next() {
		var e Event
		if err = rows.Scan(&e.Seq, &e.Project, &e.User, &e.Object, &e.Action, &e.Created, &e.Actor, &e.ActorName); err != nil {
			return err
		}
		items = append(items, e)
	}
	if err = rows.Err(); err == nil {
		write(w, 200, items)
	}
	return err
}
