# Newflasher reference and attribution

The S1 signature/download/chunk/erase/flash/session behavior and Windows transport
in this directory are adapted from `munjeni/newflasher`, commit
`59f12e437d29f0385eb27dcebba5158aa3f97b45`, version 61.

Source: https://github.com/munjeni/newflasher/tree/59f12e437d29f0385eb27dcebba5158aa3f97b45

Reference `newflasher.c` SHA-256:
`79804a4df5a35e507a96df827083ca3eac3f70d068223ca2a70246f4ad2f9e17`.

| Rust module | Upstream reference | Deliberate differences |
|---|---|---|
| `protocol.rs` | `get_reply`, `process_sins`, session flag in `main` | Bounded responses/deadlines, strict DATA lengths, no automatic legacy-to-modern signature fallback, raw device failures withheld from public logs |
| `engine.rs` | `process_sins`, flash session enter/exit, Sync | Rehash/preflight before I/O, explicit preselected targets, durable intent/ACK evidence, cancellation at acknowledged boundaries, no reboot or partial resume |
| `transport/windows.rs` | `open_dev`, Windows `transfer_bulk_async`, CreateFile block | Native Rust handle ownership, CancelIoEx followed by completion wait, no driver extraction/installation |
| `sin.rs` | `process_sins`, TAR checksum helpers, gzip paths | Streaming/no extraction, strict regular members and CMS/chunk order, gzip trailer checks, resource bounds |
| `package.rs`, `policy.rs` | `check_in_updatexml`, XML callbacks and file filtering | Structural XML parsing, explicit data/modem/DSP preservation, unknown targets blocked; no unvalidated profile enables writes |

No Sony firmware or driver installer is bundled here. Protocol test signatures
are synthetic and cannot authenticate firmware to a real device.

## MIT license

Copyright (C) 2017 Munjeni

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
