# Original stopped-container gray pixels

`original-gray.json` is emitted by `crates/core/tests/java/IconGrayGolden.java`, calling the original `ConnectModule.createGrayImage` on 16 synthetic RGBA pixels. It does not read saved connections or contact hosts. SHA256: `88D8E98D46D5739E6609AC0CAA9BAFD2EB8FA7BFA0A4B8C5EB1A386FC470DFB1`.

The conversion applies `(299 R + 587 G + 114 B) / 1000`, preserves alpha, and runs before scaling and premultiplication. This fixture describes the original stopped-container conversion; it does not imply every faded tree icon uses the same transform.
