.PHONY: clean serve
default: clean

clean:
	cargo fmt && cargo clippy

serve:
	fuser -k 8089/tcp || true && cargo run
