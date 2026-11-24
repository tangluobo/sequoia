# libunftp 0.23.1

Vendored from the Cargo registry archive whose SHA-256 is
`3dc54288ecbf8beefa5cab3a9921ab03bf1f18a3a0872cbcdb5313fc0baebaeb`.
The original Apache 2.0 license and copyright are included.

Tomato enables IPv4 active FTP. The patched PORT handler checks the control
peer and rejects privileged ports before dialing. Passive listeners and
transfers belong to the control connection and stop when replaced, aborted,
or closed. Passive listeners reject other peers and avoid Winsock address
reuse. The command parser also accepts the legacy XRMD alias.
