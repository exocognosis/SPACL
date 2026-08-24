#!/usr/bin/env python3
"""Render a shareable terminal video from a real SPACL demo run."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
import textwrap
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


WIDTH = 1280
HEIGHT = 720
FPS = 30
BACKGROUND = "#070b16"
WINDOW = "#0f172a"
CHROME = "#172033"
TEXT = "#dbeafe"
MUTED = "#94a3b8"
CYAN = "#22d3ee"
GREEN = "#4ade80"
YELLOW = "#facc15"
RED = "#fb7185"


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path("target/release/spacl"),
        help="SPACL binary to record",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("docs/demo/spacl-multi-agent-demo.mp4"),
        help="MP4 output path",
    )
    parser.add_argument(
        "--poster",
        type=Path,
        default=Path("docs/demo/spacl-multi-agent-demo-poster.png"),
        help="README poster output path",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("docs/demo/manifest.json"),
        help="Export manifest output path",
    )
    return parser.parse_args()


def find_font() -> Path:
    candidates = [
        Path("/System/Library/Fonts/SFNSMono.ttf"),
        Path("/System/Library/Fonts/Menlo.ttc"),
        Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
        Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    raise SystemExit("No supported monospaced font was found.")


def select_line(lines: list[str], prefix: str, occurrence: int = 0) -> str:
    matches = [line for line in lines if line.strip().startswith(prefix)]
    if len(matches) <= occurrence:
        raise SystemExit(f"Demo output did not contain required line: {prefix}")
    return matches[occurrence].strip()


def color_for(line: str) -> str:
    if line.startswith("$") or line.startswith("task assigned") or line.startswith("token distributed"):
        return CYAN
    if line.startswith("invalid token rejected"):
        return RED
    if line.startswith("approval assertions"):
        return YELLOW
    if (
        line.startswith("valid token accepted")
        or line.startswith("action completed")
        or line.startswith("audit chains verified")
        or line.startswith("demo complete")
    ):
        return GREEN
    if line.startswith("SPACL") or line.startswith("Shared task state"):
        return TEXT
    return MUTED


def wrapped(lines: list[str], width: int = 80) -> list[str]:
    result: list[str] = []
    for line in lines:
        parts = textwrap.wrap(
            line,
            width=width,
            subsequent_indent="  ",
            replace_whitespace=False,
            drop_whitespace=True,
        ) or [""]
        result.extend(parts)
    return result


def render_scene(path: Path, title: str, lines: list[str], scene: int, total: int) -> None:
    font_path = find_font()
    font = ImageFont.truetype(str(font_path), 23)
    small_font = ImageFont.truetype(str(font_path), 17)
    title_font = ImageFont.truetype(str(font_path), 24)
    image = Image.new("RGB", (WIDTH, HEIGHT), BACKGROUND)
    draw = ImageDraw.Draw(image)

    draw.rounded_rectangle((45, 38, WIDTH - 45, HEIGHT - 38), radius=18, fill=WINDOW)
    draw.rounded_rectangle((45, 38, WIDTH - 45, 92), radius=18, fill=CHROME)
    draw.rectangle((45, 70, WIDTH - 45, 92), fill=CHROME)
    for x, color in [(72, "#fb7185"), (100, "#facc15"), (128, "#4ade80")]:
        draw.ellipse((x - 7, 58 - 7, x + 7, 58 + 7), fill=color)
    draw.text((165, 49), "SPACL — secure multi-agent demo", font=small_font, fill=MUTED)
    draw.text((75, 116), title, font=title_font, fill=TEXT)

    visible = wrapped(lines)[-20:]
    y = 164
    for line in visible:
        draw.text((75, y), line, font=font, fill=color_for(line))
        y += 25

    progress_left = 75
    progress_right = WIDTH - 75
    progress_y = HEIGHT - 72
    draw.rounded_rectangle(
        (progress_left, progress_y, progress_right, progress_y + 5), radius=2, fill="#273449"
    )
    fraction = (scene + 1) / total
    draw.rounded_rectangle(
        (
            progress_left,
            progress_y,
            progress_left + int((progress_right - progress_left) * fraction),
            progress_y + 5,
        ),
        radius=2,
        fill=CYAN,
    )
    draw.text(
        (75, HEIGHT - 53),
        "github.com/exocognosis/SPACL",
        font=small_font,
        fill=MUTED,
    )
    draw.text(
        (WIDTH - 185, HEIGHT - 53),
        f"{scene + 1:02d} / {total:02d}",
        font=small_font,
        fill=MUTED,
    )
    image.save(path, optimize=True)


def main() -> None:
    args = arguments()
    binary = args.binary.resolve()
    if not binary.exists():
        raise SystemExit(f"SPACL binary does not exist: {binary}")
    ffmpeg = shutil.which("ffmpeg")
    ffprobe = shutil.which("ffprobe")
    if not ffmpeg or not ffprobe:
        raise SystemExit("ffmpeg and ffprobe are required.")

    with tempfile.TemporaryDirectory(prefix="spacl-video-") as temporary:
        workspace = Path(temporary)
        demo_output = workspace / "demo"
        command = [
            str(binary),
            "--no-color",
            "--compact",
            "demo",
            "--output",
            str(demo_output),
            "--watch",
        ]
        result = subprocess.run(command, check=True, capture_output=True, text=True)
        lines = result.stdout.splitlines()

        task_lines = [select_line(lines, "task assigned", index) for index in range(3)]
        distribution_lines = [
            select_line(lines, "token distributed", index) for index in range(3)
        ]
        accepted_lines = [
            select_line(lines, "valid token accepted", index) for index in range(3)
        ]
        completed_lines = [select_line(lines, "action completed", index) for index in range(3)]
        invalid_line = select_line(lines, "invalid token rejected")
        approval_line = select_line(lines, "approval assertions")
        coordinator_audit = select_line(lines, "coordinator records=")
        robot_audits = [select_line(lines, f"sim-robot-{index} records=") for index in range(1, 4)]

        stages: list[tuple[str, list[str], float]] = [
            (
                "Hybrid-signed coordination for three simulated robots",
                ["SPACL SECURE MULTI-AGENT LOOP", "", "Issue → verify → execute → audit"],
                5.0,
            ),
            (
                "Run one command",
                ["$ spacl --no-color demo --watch"],
                5.0,
            ),
            (
                "1. Assign task ownership",
                ["$ spacl --no-color demo --watch", task_lines[0]],
                5.0,
            ),
            (
                "2. Distribute a signed action token",
                [task_lines[0], distribution_lines[0]],
                5.0,
            ),
            (
                "3. Reject modified claims",
                [distribution_lines[0], invalid_line],
                7.0,
            ),
            (
                "4. Accept only the valid token",
                [invalid_line, accepted_lines[0], completed_lines[0]],
                5.0,
            ),
            (
                "Robot 2: high-risk action",
                [task_lines[1], approval_line, distribution_lines[1]],
                6.0,
            ),
            (
                "Robot 2: verified execution",
                [accepted_lines[1], completed_lines[1]],
                5.0,
            ),
            (
                "Robot 3: independent execution gate",
                [task_lines[2], distribution_lines[2], accepted_lines[2], completed_lines[2]],
                6.0,
            ),
            (
                "Shared task state",
                [
                    "Shared task state",
                    "warehouse-task-1 → sim-robot-1",
                    "warehouse-task-2 → sim-robot-2",
                    "warehouse-task-3 → sim-robot-3",
                ],
                6.0,
            ),
            (
                "Signed hash chains verify",
                ["audit chains verified", coordinator_audit, *robot_audits],
                7.0,
            ),
            (
                "Complete physical accountability loop",
                [
                    "demo complete",
                    "3 valid tokens accepted",
                    "1 tampered token rejected",
                    "4 signed audit chains verified",
                    "",
                    "github.com/exocognosis/SPACL",
                ],
                8.0,
            ),
        ]
        duration = sum(stage[2] for stage in stages)
        if not 60.0 <= duration <= 90.0:
            raise SystemExit(f"Video duration must be 60–90 seconds, got {duration}")

        frame_dir = workspace / "frames"
        frame_dir.mkdir()
        frame_paths: list[Path] = []
        for index, (title, stage_lines, _) in enumerate(stages):
            frame_path = frame_dir / f"scene-{index:02d}.png"
            render_scene(frame_path, title, stage_lines, index, len(stages))
            frame_paths.append(frame_path)

        concat_path = workspace / "frames.txt"
        concat_lines: list[str] = []
        for frame_path, (_, _, stage_duration) in zip(frame_paths, stages):
            concat_lines.append(f"file '{frame_path}'")
            concat_lines.append(f"duration {stage_duration:.3f}")
        concat_lines.append(f"file '{frame_paths[-1]}'")
        concat_path.write_text("\n".join(concat_lines) + "\n", encoding="utf-8")

        args.output.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            [
                ffmpeg,
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
                str(concat_path),
                "-t",
                f"{duration:.3f}",
                "-vf",
                f"fps={FPS},format=yuv420p",
                "-c:v",
                "libx264",
                "-preset",
                "slow",
                "-crf",
                "20",
                "-movflags",
                "+faststart",
                str(args.output),
            ],
            check=True,
        )
        args.poster.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(frame_paths[10], args.poster)

    probe = json.loads(
        subprocess.run(
            [
                ffprobe,
                "-v",
                "error",
                "-show_entries",
                "format=duration:stream=width,height,codec_name",
                "-of",
                "json",
                str(args.output),
            ],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    digest = hashlib.sha256(args.output.read_bytes()).hexdigest()
    manifest = {
        "artifact": str(args.output),
        "poster": str(args.poster),
        "source": "output from the real SPACL three-robot demo command",
        "source_command": "spacl --no-color --compact demo --output <temporary> --watch",
        "duration_seconds": round(float(probe["format"]["duration"]), 3),
        "width": probe["streams"][0]["width"],
        "height": probe["streams"][0]["height"],
        "video_codec": probe["streams"][0]["codec_name"],
        "audio": False,
        "sha256": digest,
    }
    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
