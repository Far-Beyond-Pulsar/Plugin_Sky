# Deep-Space Skybox (galaxy and nebula layers)

- Creator: Toolbench (https://toolbenchdev.com), made from scratch with Blender
  procedural shader nodes; no photos, scans or third-party assets.
- Source: https://toolbenchdev.com/gamedev/cc0-game-assets/deep-space-skybox/
- Download: https://toolbenchdev.com/downloads/deep-space-skybox-layers-1.0.zip
- License: CC0 1.0 Universal (public domain dedication). Commercial use,
  redistribution and modification are permitted; credit is not required.
  Legal code: `LICENSE-CC0-1.0.txt` (copied from the upstream archive).

Only the `galaxy` and `nebula` layers are used. They are separate from the
`stars` layer, so they are starless: the engine's procedural stars, shooting
stars and directional-light moon stay independently controllable on top.

These files are copied into a project's `assets/sky/deep-space/` when the
author chooses Create > "Sky Panoramas (CC0 Deep-Space)" in the editor. The
plugin downloads the archive, verifies its SHA-256 and writes the two
4096x2048 Radiance HDR files unchanged, plus a combined file (galaxy with the
nebula brightened and added) derived from them. They are not stored in this
repository and are not embedded in any binary.
