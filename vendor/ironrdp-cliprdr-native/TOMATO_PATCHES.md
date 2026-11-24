# Tomato changes

Add owner-thread message callback for lazy OLE file publication independent of GPU redraw. Notify file-list completion. Observe clipboard changes while Tomato is active, with origin/sequence suppression in the adapter. Destroy the hidden HWND and release subclass context; clean up constructor failures.

Mark the owner as closing before DestroyWindow. Synchronous WM_RENDERALLFORMATS/WM_RENDERFORMAT destruction callbacks do not start remote requests after that point; live owners retain normal delayed rendering. Shutdown does not call EmptyClipboard or replace any other application's clipboard contents. Windows retains data already materialized. The closing marker is an owner-thread Rc<Cell<bool>> and does not move the HWND or its context across threads. TOMATO_CAPTURE_TRACE optionally records teardown stages without clipboard payloads.

Native clipboard destruction tests use a separate child process with its own private window station and desktop. Its real delayed-rendered clipboard is isolated from the interactive user's clipboard; the existing hidden-window owner-release test still changes no clipboard contents.
