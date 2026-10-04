Native EFS/NV wire compatibility is adapted from [JohnBel/EfsTools](https://github.com/JohnBel/EfsTools), specifically `Utils/HdlcEncoder.cs`, `Utils/Crc16.cs`, `Qualcomm/QcdmCommands`, `QcdmManagers`, `FileUtils`, `PathUtils`, and `NvItemUtils`. The decimal constants, ASCII password, PUT allocation and range-mask layouts are intentional compatibility details. Strict CRC, streaming receive, error checking and cancellation are new Rust behavior.

The MIT License (MIT)

Copyright (c) 2018-2020 JohnBel

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

`fixtures/upstream.json` is produced by invoking the cached C# assembly's encoders through the offline reflection harness in `scripts/efs-golden`. No serial port or modem manager is constructed. .NET is a developer-only fixture regeneration tool; shipping/building/running the native app does not require it.

`fixtures/stock-presets.json` records relative filenames, lengths and content hashes from the user's stock beta11 balance/performance assets. It includes no preset payloads, device dump, IMEI or unlock code. The user's shipped presets are never edited or bundled. Firmware images are not included or redistributed.
