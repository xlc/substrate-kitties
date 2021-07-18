run-tmp:
	cargo run -- --dev --tmp

run:
	cargo run -- --dev

toolchain:
	./scripts/init.sh

build:
	cargo build

check:
	SKIP_WASM_BUILD= cargo check --all --tests

test:
	SKIP_WASM_BUILD= cargo test --all

purge:
	cargo run -- purge-chain --dev -y

restart: purge run

init: toolchain build-full

benchmark:
	cargo run --manifest-path node/Cargo.toml --features runtime-benchmarks -- benchmark --extrinsic '*' --pallet '*' --execution=wasm --wasm-execution=compiled

test-benchmark:
	cargo test --manifest-path pallets/kitties/Cargo.toml --features runtime-benchmarks -- --nocapture
