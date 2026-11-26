# YAML plug-in changelog

Every release of the YAML plug-in (`plugins/yaml`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-11-13

Needs Zen IDE 26.11 or later.

### Added

- Schemas for GitHub workflow files (`.github/workflows/*.yml`) and for Kubernetes 1.34 manifests
  (`*.k8s.yaml`, and the YAML files under a `k8s` or `kubernetes` folder), shipped in the plug-in:
  the YAML language server completes and checks those files offline.

### Changed

- The YAML language server downloads no schemas: not the public schema catalog, and not a schema a
  file names with a web address.

## 0.1.0 - 2026-10-16

### Added

- The first release: highlighting for YAML files, also Docker Compose files and Kubernetes
  manifests: keys, strings, numbers, constants, comments, anchors, tags, block scalars and
  `${VAR}` interpolation.
