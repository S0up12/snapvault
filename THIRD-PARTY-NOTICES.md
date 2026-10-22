# Third-party notices

Snapvault itself is released under the MIT License (see `LICENSE`).

## FFmpeg

The Windows installer bundles unmodified FFmpeg binaries (`ffmpeg.exe` and `ffprobe.exe`), taken from the gyan.dev "release essentials" build. As configured by that build (`--enable-gpl --enable-version3 --enable-libx264 --enable-libx265`), these binaries are licensed under the **GNU General Public License version 3 (GPLv3)**.

Snapvault runs them only as separate programs (it starts them as child processes and reads their output); it does not link against FFmpeg libraries. The FFmpeg license text is included with the installer as `licenses/FFMPEG-LICENSE.txt`.

- FFmpeg license information: https://ffmpeg.org/legal.html
- FFmpeg source code: https://ffmpeg.org/download.html#get-sources
- Build used and its source/build details: https://www.gyan.dev/ffmpeg/builds/
- GPLv3 text: https://www.gnu.org/licenses/gpl-3.0.html

**Written offer for source:** on request, we will provide the complete corresponding source for the bundled FFmpeg binaries (or a pointer to the exact upstream release). Open an issue at https://github.com/S0up12/snapvault/issues.

You can use your own FFmpeg instead: if `ffmpeg`/`ffprobe` are on your PATH and no bundled copy is present (for example in a development build), Snapvault uses those.

## Other dependencies

Snapvault is built with Tauri, React and other open-source libraries. Their exact versions are pinned in `package-lock.json` (JavaScript) and `src-tauri/Cargo.lock` (Rust); each package carries its own license in its source distribution.
