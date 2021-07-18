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

benchmark-output:
	cargo run --manifest-path node/Cargo.toml --release --features runtime-benchmarks -- benchmark --extrinsic '*' --pallet pallet_kitties --output runtime/src/weights/pallet_kitties.rs --execution=wasm --wasm-execution=compiled

benchmark-traits:
	cargo run --manifest-path node/Cargo.toml --release --features runtime-benchmarks -- benchmark --extrinsic '*' --pallet pallet_kitties --output pallets/kitties/src/weights.rs --template=frame-weight-template.hbs --execution=wasm --wasm-execution=compiled

test-benchmark:
	cargo test --manifest-path pallets/kitties/Cargo.toml --features runtime-benchmarks -- --nocapture
