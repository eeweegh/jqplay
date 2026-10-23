default: check

check: fmt-check lint test

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

lint:
    cargo clippy --all-targets --locked -- -D warnings

test:
    cargo test --locked

# synthetic ~5MB JSON for performance checks (written to the git-ignored target/)
fixture count="18000":
    mkdir -p target
    jq -n '[range({{count}}) | {id: ., name: "item-\(.)", active: (. % 2 == 0), score: (. * 1.5), tags: ["alpha","beta","gamma"], owner: {id: (. % 97), email: "user\(. % 97)@example.com"}, notes: "lorem ipsum dolor sit amet \(.)"}]' > target/fixture.json

perf count="18000": (fixture count)
    cargo run --release -- target/fixture.json --color

run *args:
    cargo run --release -- {{args}}

build:
    cargo build --release --locked

completions shell="zsh":
    cargo run --quiet -- --completions {{shell}}

man:
    cargo run --quiet -- --man

install:
    cargo install --path . --locked --root ${HOME}/.local
