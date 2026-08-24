# Configuration

Run `spacl init` to generate a coordinator file and a sample robot file. SPACL uses TOML.

## Coordinator

```toml
bind = "127.0.0.1:8080"
data_dir = "/absolute/path/to/workspace/data/coordinator"
```

Start it:

```bash
spacl coordinator --config /path/to/coordinator.toml
```

## Robot Runtime

```toml
robot_id = "robot-1"
identity = "/absolute/path/to/workspace/secrets/robot-1.identity.json"
coordinator_public = "/absolute/path/to/workspace/data/coordinator/coordinator.public.json"
bind = "127.0.0.1:8081"
data_dir = "/absolute/path/to/workspace/data/robot-1"
```

Start it:

```bash
spacl robot --config /path/to/robot-1.toml
```

Command-line values override matching config values. The global `--data-dir` option sets the workspace root. The `SPACL_DATA_DIR` environment variable sets the same value.

SPACL uses the operating system local data directory when neither value is present. Run `spacl status` to print the selected workspace path.

Do not store private identity material in a TOML file. Store only the private identity path.
