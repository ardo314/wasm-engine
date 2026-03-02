# math

A WebAssembly Component mathematics library implemented in Rust, providing comprehensive linear algebra and geometric transformation utilities.

## Overview

This library provides a collection of mathematical types and operations commonly used in 3D graphics, robotics, and computational geometry. The implementations are inspired by [three.js](https://github.com/mrdoob/three.js/tree/dev/src/math) and compiled as a WebAssembly Component for cross-platform compatibility.

**Package:** `ardo314:math@0.0.3`

## Features

### Vector Operations

- **Vector2d** - 2D vector operations (addition, subtraction, dot product, normalization, etc.)
- **Vector3d** - 3D vector operations including cross product
- **Vector4d** - 4D vector operations for homogeneous coordinates

### Matrix Operations

- **Matrix2x2** - 2D transformation matrices
- **Matrix3x3** - 3D rotation and scaling matrices
- **Matrix4x4** - 4D transformation matrices for 3D graphics

### Rotation Representations

- **Quaternion** - Quaternion-based rotations with SLERP interpolation
- **AxisAngle** - Axis-angle rotation representation
- **RotationMatrix2x2** - 2D rotation matrices
- **RotationMatrix3x3** - 3D rotation matrices
- **RotationVector** - Rotation vectors (scaled axis representation)

### Geometric Types

- **Point2d** - 2D point operations
- **Point3d** - 3D point operations
- **Pose2d** - 2D pose (position + orientation)
- **Pose3d** - 3D pose (position + orientation)

## Building

This project uses WebAssembly Component tooling. Build the component with:

```bash
cargo component build --release
```

## Configuration

The project uses `cargo-component` with custom configuration in `config.toml` for registry settings.

### Available Tasks

- **Update dependencies**: `cargo component update --config config.toml`
- **Generate bindings**: `cargo component bindings --config config.toml`
- **Overwrite wkg config**: Copies config.toml to `~/.config/wasm-pkg/`

## Development

The library is built as a WebAssembly Component following the Component Model specification, making it usable across different programming languages and environments that support WASM components.

### Testing

Run tests with:

```bash
cargo test
```

## Acknowledgments

Most of the mathematical implementations are adapted from [three.js](https://github.com/mrdoob/three.js/tree/dev/src/math).
