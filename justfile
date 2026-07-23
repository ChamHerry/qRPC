set shell := ["bash", "-cu"]
python := `if [ -x /opt/miniconda3/bin/python3.13 ]; then printf /opt/miniconda3/bin/python3.13; else printf python3; fi`

build:
    cargo build --workspace

test:
    cargo test --workspace

bench:
    cargo bench -p qrpc-core

test-interop:
    {{python}} tests/interop/runner.py

verify:
    cargo build --workspace
    cargo test --workspace
