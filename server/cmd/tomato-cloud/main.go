package main

import (
	"bufio"
	"context"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/tangluobo/tomato/cloud/internal/collaboration"
	"golang.org/x/term"
)

func main() {
	data := flag.String("data", "./cloud-data", "persistent data directory")
	listen := flag.String("listen", "127.0.0.1:8443", "listen address")
	cert := flag.String("tls-cert", "", "TLS certificate PEM")
	key := flag.String("tls-key", "", "TLS private key PEM")
	proxy := flag.Bool("behind-proxy", false, "allow HTTP behind a TLS reverse proxy on a private network")
	register := flag.Bool("registration", false, "allow account self-registration")
	fixtureRegistration := flag.Bool("local-fixture-registration", false, "allow unverified accounts for loopback-only isolated tests")
	publicURL := flag.String("public-url", os.Getenv("TOMATO_CLOUD_PUBLIC_URL"), "public HTTPS URL used in account verification links")
	command := flag.String("command", "serve", "serve, create-admin, reset-password, backup")
	email := flag.String("email", "", "account email for local account commands")
	output := flag.String("output", "", "new backup directory")
	flag.Parse()
	var mailer collaboration.Mailer
	if host := os.Getenv("TOMATO_CLOUD_SMTP_ADDRESS"); host != "" {
		password := os.Getenv("TOMATO_CLOUD_SMTP_PASSWORD")
		if file := os.Getenv("TOMATO_CLOUD_SMTP_PASSWORD_FILE"); file != "" {
			value, e := os.ReadFile(file)
			if e != nil {
				log.Fatal("unable to read SMTP password file")
			}
			if len(value) > 4096 {
				log.Fatal("SMTP password file too large")
			}
			password = strings.TrimRight(string(value), "\r\n")
		}
		mailer = collaboration.SMTP{Address: host, Username: os.Getenv("TOMATO_CLOUD_SMTP_USERNAME"), Password: password, From: os.Getenv("TOMATO_CLOUD_SMTP_FROM")}
	}
	if *register && (mailer == nil || !strings.HasPrefix(*publicURL, "https://")) {
		log.Fatal("public registration requires STARTTLS SMTP and a public HTTPS URL")
	}
	if *fixtureRegistration {
		host, _, e := net.SplitHostPort(*listen)
		ip := net.ParseIP(host)
		if e != nil || *proxy || (host != "localhost" && (ip == nil || !ip.IsLoopback())) {
			log.Fatal("fixture registration is loopback-only and cannot use a reverse proxy")
		}
	}
	server, err := collaboration.Open(collaboration.Config{DataDir: *data, Registration: *register || *fixtureRegistration, Mailer: mailer, PublicURL: *publicURL})
	if err != nil {
		log.Fatal(err)
	}
	defer server.Close()
	switch *command {
	case "create-admin", "reset-password":
		fmt.Fprintln(os.Stderr, "Read password from standard input (minimum 12 bytes).")
		reader := bufio.NewReaderSize(io.LimitReader(os.Stdin, 1026), 2048)
		var p string
		if term.IsTerminal(int(os.Stdin.Fd())) {
			value, e := term.ReadPassword(int(os.Stdin.Fd()))
			p = string(value)
			err = e
			fmt.Fprintln(os.Stderr)
		} else {
			p, err = reader.ReadString('\n')
		}
		if err != nil && len(p) == 0 {
			log.Fatal("password input failed")
		}
		p = strings.TrimRight(p, "\r\n")
		if *command == "create-admin" {
			err = server.CreateAdmin(*email, *email, p)
		} else {
			err = server.ResetPassword(*email, p)
		}
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println("Account updated.")
		return
	case "backup":
		if err = server.Backup(*output); err != nil {
			log.Fatal(err)
		}
		fmt.Println("Backup completed.")
		return
	case "serve":
	default:
		log.Fatal("unknown command")
	}
	host, _, err := net.SplitHostPort(*listen)
	if err != nil {
		log.Fatal(err)
	}
	ip := net.ParseIP(host)
	if *cert == "" && !(*proxy || host == "localhost" || (ip != nil && ip.IsLoopback())) {
		log.Fatal("non-loopback HTTP requires TLS or an explicitly configured TLS reverse proxy")
	}
	stop, shutdown := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer shutdown()
	httpServer := &http.Server{Addr: *listen, Handler: server.Handler(), ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 15 * time.Second, WriteTimeout: 40 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 16384, BaseContext: func(net.Listener) context.Context { return stop }}
	done := make(chan struct{})
	go func() {
		defer close(done)
		<-stop.Done()
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = httpServer.Shutdown(ctx)
	}()
	listener, err := net.Listen("tcp", *listen)
	if err != nil {
		log.Fatal(err)
	}
	log.Printf("Tomato Cloud listening at %s", listener.Addr())
	if *cert != "" {
		err = httpServer.ServeTLS(listener, *cert, *key)
	} else {
		err = httpServer.Serve(listener)
	}
	if err != nil && err != http.ErrServerClosed {
		log.Fatal(err)
	}
	<-done
}
