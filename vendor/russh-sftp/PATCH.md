Upstream: russh-sftp 3.0.1, Apache-2.0.

Only src/client/session.rs adds read_dir_limited(path, max_entries).
It accumulates linearly, rejects a page exceeding the requested total and
closes the directory handle on success or error. Existing APIs are unchanged.
The application uses 20,000 entries per directory, four in-flight reads
and writes, and the upstream 256 KiB packet limit. This prevents the GUI
limit from being applied only after the library has collected a whole
unbounded directory. Protocol encoding and crypto are unchanged.
