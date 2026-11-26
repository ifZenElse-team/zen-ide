# npm plug-in changelog

Every release of the npm plug-in (`plugins/npm`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-06

### Added

- The first release, for projects with a `package.json`; WebWave comes with it, and every other IDE
  offers it in a notification when such a project opens. It is also in the Marketplace.
- The npm tool window at the right: the `package.json` of the project, of its workspaces and of the
  folders at its root, each with its scripts and the package manager it uses (npm, pnpm, Yarn or
  Bun). Double-click a script to run it, with its output in the Run tool window.
- Install Dependencies, the only action here that uses the network, and only when you press it;
  Toggle Offline Mode keeps installs off the network.
