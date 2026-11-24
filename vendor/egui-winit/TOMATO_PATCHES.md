# Tomato patches

Native cursor cache retains the actual image allocation and compares size/hotspot as well as Arc identity. A freed bitmap address cannot select a stale cursor. The RDP panel now uses the existing native cursor bridge rather than a GPU-painted pointer.

Applying `ViewportCommand::Close` requests a native redraw of the target window after installing its close event. The command's earlier egui repaint request may already have been coalesced; a busy detached RDP viewport must not leave the root window waiting to consume that event. The next normal integration pass still decides whether to honor `CancelClose`; this does not forcibly terminate windows or the process. Continuous-image native capture runs, rather than a mocked redraw call, validate the shutdown behavior.

Read the current Windows monitor visual work area through GetMonitorInfoW. The optional physical-pixel rectangle is forwarded with viewport input so native dialogs can center inside their owner display without covering taskbars. Monitor DPI and native outer size are handled in the UI placement adapter; no display or taskbar settings are changed. Other platforms leave the optional field absent.
