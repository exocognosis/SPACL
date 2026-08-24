set dotenv-load

data_dir := env_var_or_default("SPACL_DATA_DIR", ".spacl-dev")

help:
    cargo run -- --help

init:
    cargo run -- --data-dir "{{ data_dir }}" init

single-agent:
    cargo run -- --data-dir "{{ data_dir }}" single-agent --watch

demo:
    cargo run -- --data-dir "{{ data_dir }}" demo --watch

demo-video:
    cargo build --release --locked
    python3 scripts/render_demo_video.py

coordinator:
    cargo run -- --data-dir "{{ data_dir }}" coordinator --config "{{ data_dir }}/config/coordinator.toml"

robot1:
    cargo run -- --data-dir "{{ data_dir }}" robot --config "{{ data_dir }}/config/robot-1.toml"

status:
    cargo run -- --data-dir "{{ data_dir }}" status --robot-url http://127.0.0.1:8081

test:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features --locked -- -D warnings
    cargo test --all-targets --locked

docs:
    cargo doc --no-deps --locked

docker:
    docker build -t spacl:dev .
