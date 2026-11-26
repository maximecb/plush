# Editing tremor maps in TrenchBroom

Tremor maps are in the Valve 220 format that TrenchBroom writes.

## Setup

1. Copy this folder to TrenchBroom's games folder, under the name `Tremor`:
   - macOS: `~/Library/Application Support/TrenchBroom/games/Tremor/`
   - Linux: `~/.TrenchBroom/games/Tremor/`
   - Windows: `%AppData%\TrenchBroom\games\Tremor\`
2. In TrenchBroom's preferences, pick Tremor and set its game path to
   `examples/tremor`, the folder that holds `textures/`.
3. Make a new map for Tremor, or open one from `examples/tremor/maps/`, and
   run it with `plush examples/tremor/main.psh path/to/map.map`.

## Conventions

- One unit is 2.5 cm, 40 units to the meter, and +Z is up. A texture at
  scale 1 shows one texel per unit, which is the game's usual density.
- Faces textured `skip` are not drawn. They still bound the brush.
- Textures named `lava`, `lava_*`, `water` or `water_*` swim and burn at full
  brightness.
- Textures named `light_*` show at full brightness, so the lit face of a lamp
  glows while the rest of the lamp takes light from the bake like anything
  else.
- `func_brush` holds level geometry that needs settings of its own: a `light`
  level to show at instead of a baked one, an `alpha` below 1 to make it
  see-through, or a `flow` of 0 or 1 that overrides the texture name.
- `func_door` makes each of its brushes a door, filling the brush's box when
  shut and rising by `lift` units to open.
- `func_elevator` makes each of its brushes a platform that starts at the
  top, goes down by `travel` units at `speed` units per second, and back up.
  It sets off a second after the player steps on, and comes to them when they
  wait at the other end. Whatever stands on it rides with it.
- `info_player_start` stands the player at its origin. Entities stand with
  their base at their origin, and `angle` gives their facing in degrees.
  The `sword` stands point down with its tip at its origin instead, and
  takes `angles`, pitch, yaw and roll, so that it can be driven into rock at
  a slant.
