# Remus

Remus is a fork of [brepkit](https://github.com/andymai/brepkit) by andymai.
It is a B-Rep modeling engine written in Rust and compiled to WebAssembly.

Remus handles NURBS geometry, boolean operations, filleting, tessellation, and
data exchange — in memory-safe Rust with first-class WASM support.

## Why Remus?

- **Pure Rust** — no C/C++ dependencies, no complex build systems
- **WASM-first** — designed for browser and Node.js environments
- **Memory-safe** — no undefined behavior, no use-after-free
- **Layered architecture** — clean separation of math, topology, operations, and I/O
- **Modern tooling** — strict linting, property-based testing, comprehensive CI
