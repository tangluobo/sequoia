# Tomato CPAL patch

CPAL 0.17.3, registry source, Apache-2.0 license (see LICENSE).

WASAPI device enumeration is now local to the calling COM apartment/thread.
The upstream process-wide OnceLock retained IMMDeviceEnumerator after its first
short-lived RDP worker thread had called CoUninitialize. Later connections
dereferenced its stale vtable, reproducible even with serial handshake tests.
The thread-local enumerator is created after the existing COM guard and released
before that guard on thread exit. No process-wide Send/Sync interface wrapper is
needed. Device discovery and playback preferences remain unchanged.

Regression: tomato-services/tests/audio_device_lifecycle.rs exercises sequential
and concurrent thread teardown without playing or recording any sound.
