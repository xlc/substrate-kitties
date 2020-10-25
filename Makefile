run-tmp:
	SKIP_WASM_BUILD= cargo run -- --dev --tmp -lruntime=debug

run:
	SKIP_WASM_BUILD= cargo run -- --dev -lruntime=debug

toolchain:
	./scripts/init.sh

build:
	cargo build

check:
	SKIP_WASM_BUILD= cargo check --all --tests

test:
	SKIP_WASM_BUILD= cargo test --all

purge:
	SKIP_WASM_BUILD= cargo run -- purge-chain --dev -y

restart: purge run

init: toolchain build-full

benchmark:
	cargo run --manifest-path node/Cargo.toml --features runtime-benchmarks -- benchmark --extrinsic '*' --pallet '*'

benchmark-output:
	cargo run --manifest-path node/Cargo.toml --release --features runtime-benchmarks -- benchmark --extrinsic '*' --pallet pallet_kitties --output runtime/src/weights --execution=wasm

benchmark-traits:
	cargo run --manifest-path node/Cargo.toml --release --features runtime-benchmarks -- benchmark --extrinsic '*' --pallet pallet_kitties --output pallets/kitties/src/weights.rs --template=frame-weight-template.hbs

test-benchmark:
	cargo test --manifest-path pallets/kitties/Cargo.toml --features runtime-benchmarks -- --nocapture
