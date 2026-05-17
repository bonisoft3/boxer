// services/boxer/bayt.cue — bayt configuration for boxer.
//
// Rust binary; no language stack exists for Rust in plugins/bayt/stacks
// yet, so this composes mise + sayt verbs directly with hand-written
// cargo commands (same shape as services/tracker-tx, which is also
// stack-less). The hand-maintained top-level Dockerfile / compose.yaml
// stay put — bayt emits the canonical Taskfiles + per-target manifests
// + Dockerfiles into .bayt/ alongside.
//
// Verbs covered: setup / doctor / build / test / integrate / generate.
// launch + release + lint stay in .say.yaml as direct cargo
// invocations (cargo run / goreleaser / cargo clippy) — no need for
// bayt artifacts there.
package boxer

import (
	bayt "bonisoft.org/plugins/bayt/core:bayt"
	mise "bonisoft.org/plugins/bayt/stacks/mise"
	sayt "bonisoft.org/plugins/bayt/stacks/sayt"
)

_boxer: bayt.#project & {
	dir:      "services/boxer"
	activate: "mise x --"

	targets: {
		"setup": sayt.setup & mise.install & {
			// gcc for rust crates with C build scripts (log, proc-macro2,
			// aws-lc-rs, ...); opensuse/leap base has no `cc` by default.
			// build/test/integrate FROM-chain off setup and inherit it.
			dockerfile: bayt.nubox
			dockerfile: preamble: [
				"RUN zypper -n install gcc=15-160000.2.2",
			]
		}
		"doctor": sayt.doctor & mise.doctor

		// Build = `cargo build`. Sources are src/ + Cargo manifests;
		// tests/ stays out so unit/integration test edits don't
		// invalidate the build stage's COPY (same pattern as the
		// gradle stack's src/main only assemble srcs). Chain off the
		// setup stage via FROM so the mise-installed rust toolchain
		// (in /root/.local/) is present — without this, build starts
		// from a bare opensuse/leap with no cargo on PATH.
		"build": sayt.build & mise.exec & {
			srcs: globs: ["src/**/*", "Cargo.toml", "Cargo.lock"]
			outs: globs: ["target/debug/boxer"]
			cmd: "builtin": do: "cargo build"
			dockerfile: from: ref: ":setup"
		}

		// Unit tests = `cargo test --lib --bins`. Skips integration
		// tests in tests/ — those run under integrate.
		"test": sayt.test & mise.exec & {
			srcs: globs: ["src/**/*", "Cargo.toml", "Cargo.lock"]
			cmd: "builtin": do: "cargo test --lib --bins"
		}

		// Integration tests = everything in tests/. Cargo's convention
		// puts each .rs in tests/ as its own integration-test crate.
		// No host.env secret, no dind.sh wrap — these are pure cargo,
		// no docker socket needed. Chain off the build stage via FROM
		// so cargo's target/ from `cargo build` is reused — `cargo
		// test --tests` only needs to compile the test crates, not
		// rebuild the library.
		"integrate": sayt.integrate & mise.exec & {
			srcs: globs: ["tests/**/*"]
			outs: globs: ["target/debug/deps/*-*"]
			dockerfile: {
				from: ref: ":build"
			}
			cmd: "builtin": {
				do: "cargo test --tests"
				dockerfile: wrap: ""
			}
		}

		"generate": sayt.generate & {cmd: "builtin": do: "true"}
	}
}

project: _boxer

depManifestsIn: {[string]: _}
_render: (bayt.#render & {project: _boxer, depManifests: depManifestsIn})
