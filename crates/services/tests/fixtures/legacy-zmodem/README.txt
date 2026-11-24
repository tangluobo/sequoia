These 34 Java source files are an unchanged test-only copy of the original
Tomato ZMODEM implementation. provenance.json records per-file SHA-256.
They are not linked into or included in the application package.

../LegacyZmodemPeer.java provides stdio startup announcements. Its receive
FileAdapter writes without buffering: the original CustomFile combines a
buffered stream with a pre-flush file-length resume check, which otherwise
rewinds small acknowledged frames. The protocol code itself is unmodified.
The adapter consumes final OO bytes as a remote command would.

The original sender emits US-ASCII filenames; its interoperability fixture
therefore uses ASCII names. A separate CRC32 sender covers native UTF-8 names
and new zero-byte files. The original receiver's zero-byte skip path is
covered with a pre-existing empty destination.

JDK 17+ compiles and executes the protocol fixture. The separate optional
LegacySshCapture UI oracle uses the original application jar and its JDK
version (currently JDK 25+). All peers bind loopback with disposable accounts.
