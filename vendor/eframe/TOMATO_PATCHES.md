# Tomato application-owned deferred viewports

The WGPU and Glow native integrations identify immediate viewports by their
class, rather than assuming that all callback-free children are immediate.
Callback-free deferred children are dispatched to the existing application
logic/UI entry points, including logic-only passes while minimized.

Root close handling checks the actual viewport ID. A child close must never
close the application. Pending minimized input is stored by viewport ID and
discarded when a window disappears, preventing a later pass on a different
window from receiving those events.
