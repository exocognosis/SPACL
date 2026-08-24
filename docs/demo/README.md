# Demo Video

`spacl-multi-agent-demo.mp4` is a 70-second terminal video of the complete three-robot loop. `spacl-multi-agent-demo.gif` is the full-loop README embed. Both show task assignment, token distribution, invalid-token rejection, valid execution, shared task state, and audit verification.

The renderer runs the real SPACL demo and uses its output. It then creates a fixed-format 1280×720 H.264 MP4 without audio.

Rebuild the video from the repository root:

```bash
just demo-video
```

Requirements:

- Rust 1.88 or later
- Python 3 with Pillow
- FFmpeg with H.264 support

See `manifest.json` for the export properties and SHA-256 digests.
