#!/usr/bin/env python3
"""Generate a walkable glass skyscraper map for examples/tremor.psh."""

from __future__ import annotations

import argparse
from pathlib import Path


SITE_HALF = 50.0
GROUND_Y = 0.2
BUILDING_X0 = -24.0
BUILDING_X1 = 24.0
BUILDING_Z0 = -22.0
BUILDING_Z1 = 30.0

LOBBY_HEIGHT = 5.0
FLOOR_HEIGHT = 4.0
SLAB_THICKNESS = 0.3
DEFAULT_UPPER_FLOORS = 26

STAIR_RUNS = ((10.0, 14.0), (15.0, 19.0))
STAIR_Z0 = -1.0
STAIR_Z1 = 7.0
STEP_RISE = 0.2

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = REPO_ROOT / "examples" / "data" / "skyscraper.map"


def number(value: float) -> str:
    if abs(value) < 0.000_000_5:
        value = 0.0
    return f"{value:.3f}".rstrip("0").rstrip(".")


class MapWriter:
    def __init__(self) -> None:
        self.lines: list[str] = []

    def line(self, text: str = "") -> None:
        self.lines.append(text)

    def section(self, title: str) -> None:
        self.lines.extend(("", f"// {title}"))

    def box(
        self,
        x0: float,
        y0: float,
        z0: float,
        x1: float,
        y1: float,
        z1: float,
        options: str,
    ) -> None:
        coords = (
            f"x0={number(x0)} y0={number(y0)} z0={number(z0)} "
            f"x1={number(x1)} y1={number(y1)} z1={number(z1)}"
        )
        self.lines.append(f"box {coords} {options}")

    def light(
        self,
        x: float,
        y: float,
        z: float,
        radius: float,
        brightness: float,
    ) -> None:
        self.lines.append(
            f"light x={number(x)} y={number(y)} z={number(z)} "
            f"radius={number(radius)} brightness={number(brightness)}"
        )

    def render(self) -> str:
        return "\n".join(self.lines) + "\n"


def add_site(out: MapWriter) -> None:
    out.section("A 100 m by 100 m site, with the tower entrance facing south")
    out.box(
        -SITE_HALF,
        -0.6,
        -SITE_HALF,
        SITE_HALF,
        -0.4,
        SITE_HALF,
        "tex=concrete_01 scale=16",
    )
    out.box(
        -SITE_HALF,
        -0.4,
        -SITE_HALF,
        SITE_HALF,
        0.0,
        -38.0,
        "tex=asphalt_02 scale=10",
    )
    out.box(
        -SITE_HALF,
        -0.4,
        -38.0,
        SITE_HALF,
        GROUND_Y,
        -34.0,
        "tex=concrete_white scale=16",
    )
    plaza_options = "tex=concrete_01 scale=16"
    out.box(-SITE_HALF, -0.4, -34.0, SITE_HALF, GROUND_Y, BUILDING_Z0, plaza_options)
    out.box(-SITE_HALF, -0.4, BUILDING_Z1, SITE_HALF, GROUND_Y, SITE_HALF, plaza_options)
    out.box(
        -SITE_HALF,
        -0.4,
        BUILDING_Z0,
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z1,
        plaza_options,
    )
    out.box(
        BUILDING_X1,
        -0.4,
        BUILDING_Z0,
        SITE_HALF,
        GROUND_Y,
        BUILDING_Z1,
        plaza_options,
    )

    for x0 in range(-47, 48, 10):
        out.box(
            float(x0),
            0.0,
            -44.08,
            float(x0 + 5),
            0.02,
            -43.92,
            "tex=concrete_white scale=40",
        )

    # Planters give the otherwise open plaza a little depth.
    for x0, x1 in ((-45.0, -31.0), (31.0, 45.0)):
        out.box(
            x0,
            GROUND_Y,
            -29.0,
            x1,
            GROUND_Y + 0.6,
            -26.0,
            "tex=stone_large top=floor_dirt scale=20",
        )


def add_lighting(out: MapWriter, upper_floors: int) -> None:
    out.section("Baked lighting for the site, lobby, and each upper floor")
    out.light(-65.0, 145.0, -85.0, 280.0, 1.0)
    out.light(-7.0, GROUND_Y + 3.2, 2.0, 48.0, 1.1)
    for floor in range(upper_floors):
        floor_y = GROUND_Y + LOBBY_HEIGHT + floor * FLOOR_HEIGHT
        out.light(-6.0, floor_y + 2.8, 4.0, 46.0, 0.95)


def add_floor_plate(
    out: MapWriter,
    y: float,
    stair_openings: tuple[tuple[float, float], ...] = (),
) -> None:
    options = (
        "tex=concrete_white top=carpet_blue bottom=ceiling_tile "
        "scale=100"
    )
    y0 = y - SLAB_THICKNESS
    if not stair_openings:
        out.box(BUILDING_X0, y0, BUILDING_Z0, BUILDING_X1, y, BUILDING_Z1, options)
        return

    out.box(BUILDING_X0, y0, BUILDING_Z0, BUILDING_X1, y, STAIR_Z0, options)
    out.box(BUILDING_X0, y0, STAIR_Z1, BUILDING_X1, y, BUILDING_Z1, options)
    cursor = BUILDING_X0
    for opening_x0, opening_x1 in stair_openings:
        out.box(cursor, y0, STAIR_Z0, opening_x0, y, STAIR_Z1, options)
        cursor = opening_x1
    out.box(cursor, y0, STAIR_Z0, BUILDING_X1, y, STAIR_Z1, options)


def add_lobby(out: MapWriter) -> None:
    out.section("Ground-floor lobby")
    add_floor_plate(out, GROUND_Y)

    wall = "tex=concrete_white scale=80"
    glass = "tex=glass_blue light=9 alpha=0.18 fit"
    lobby_top = GROUND_Y + LOBBY_HEIGHT
    entrance_top = GROUND_Y + 3.6
    out.box(
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z1 - 0.35,
        BUILDING_X1,
        lobby_top,
        BUILDING_Z1,
        wall,
    )
    out.box(
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z0,
        BUILDING_X0 + 0.35,
        lobby_top,
        BUILDING_Z1,
        wall,
    )
    out.box(
        BUILDING_X1 - 0.35,
        GROUND_Y,
        BUILDING_Z0,
        BUILDING_X1,
        lobby_top,
        BUILDING_Z1,
        wall,
    )

    out.box(
        BUILDING_X0,
        GROUND_Y,
        BUILDING_Z0,
        -2.2,
        lobby_top - SLAB_THICKNESS,
        BUILDING_Z0 + 0.08,
        glass,
    )
    out.box(
        2.2,
        GROUND_Y,
        BUILDING_Z0,
        BUILDING_X1,
        lobby_top - SLAB_THICKNESS,
        BUILDING_Z0 + 0.08,
        glass,
    )
    out.box(
        -2.2,
        entrance_top,
        BUILDING_Z0,
        2.2,
        lobby_top - SLAB_THICKNESS,
        BUILDING_Z0 + 0.08,
        glass,
    )

    for x in (-18.0, -12.0, -6.0, 6.0, 12.0, 18.0):
        out.box(
            x - 0.09,
            GROUND_Y,
            BUILDING_Z0 - 0.02,
            x + 0.09,
            lobby_top,
            BUILDING_Z0 + 0.18,
            "tex=metal_plate scale=50",
        )


def add_perimeter_band(out: MapWriter, y: float) -> None:
    options = "tex=concrete_white scale=70"
    thickness = 0.35
    y0 = y - SLAB_THICKNESS
    y1 = y + 0.65
    out.box(
        BUILDING_X0,
        y0,
        BUILDING_Z0,
        BUILDING_X1,
        y1,
        BUILDING_Z0 + thickness,
        options,
    )
    out.box(
        BUILDING_X0,
        y0,
        BUILDING_Z1 - thickness,
        BUILDING_X1,
        y1,
        BUILDING_Z1,
        options,
    )
    out.box(
        BUILDING_X0,
        y0,
        BUILDING_Z0 + thickness,
        BUILDING_X0 + thickness,
        y1,
        BUILDING_Z1 - thickness,
        options,
    )
    out.box(
        BUILDING_X1 - thickness,
        y0,
        BUILDING_Z0 + thickness,
        BUILDING_X1,
        y1,
        BUILDING_Z1 - thickness,
        options,
    )


def add_glass_storey(out: MapWriter, y0: float, y1: float) -> None:
    options = "tex=glass_grey light=9 alpha=0.16 fit"
    pane_bottom = y0 + 0.65
    pane_top = y1 - 0.2
    inset = 0.12
    out.box(
        BUILDING_X0 + inset,
        pane_bottom,
        BUILDING_Z0,
        BUILDING_X1 - inset,
        pane_top,
        BUILDING_Z0 + 0.08,
        options,
    )
    out.box(
        BUILDING_X0 + inset,
        pane_bottom,
        BUILDING_Z1 - 0.08,
        BUILDING_X1 - inset,
        pane_top,
        BUILDING_Z1,
        options,
    )
    out.box(
        BUILDING_X0,
        pane_bottom,
        BUILDING_Z0 + inset,
        BUILDING_X0 + 0.08,
        pane_top,
        BUILDING_Z1 - inset,
        options,
    )
    out.box(
        BUILDING_X1 - 0.08,
        pane_bottom,
        BUILDING_Z0 + inset,
        BUILDING_X1,
        pane_top,
        BUILDING_Z1 - inset,
        options,
    )


def add_stairs(
    out: MapWriter,
    y0: float,
    y1: float,
    flight: int,
) -> None:
    stair_x0, stair_x1 = STAIR_RUNS[flight % len(STAIR_RUNS)]
    ascending_positive_z = flight % 2 == 0
    step_count = round((y1 - y0) / STEP_RISE)
    step_depth = (STAIR_Z1 - STAIR_Z0) / step_count
    options = "tex=concrete_white top=floor_tread scale=80 light=8"
    for step in range(step_count):
        if ascending_positive_z:
            z0 = STAIR_Z0 + step * step_depth
            z1 = STAIR_Z0 + (step + 1) * step_depth
        else:
            z0 = STAIR_Z1 - (step + 1) * step_depth
            z1 = STAIR_Z1 - step * step_depth
        out.box(
            stair_x0,
            y0,
            z0,
            stair_x1,
            y0 + (step + 1) * STEP_RISE,
            z1,
            options,
        )


def add_mullions(out: MapWriter, upper_base: float, roof_y: float) -> None:
    out.section("Continuous mullions tie the separate window storeys together")
    options = "tex=metal_plate scale=50 light=8"
    for x in (-18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0):
        out.box(
            x - 0.08,
            upper_base,
            BUILDING_Z0 - 0.1,
            x + 0.08,
            roof_y,
            BUILDING_Z0 + 0.1,
            options,
        )
        out.box(
            x - 0.08,
            upper_base,
            BUILDING_Z1 - 0.1,
            x + 0.08,
            roof_y,
            BUILDING_Z1 + 0.1,
            options,
        )
    for z in (-16.0, -10.0, -4.0, 2.0, 8.0, 14.0, 20.0, 26.0):
        out.box(
            BUILDING_X0 - 0.1,
            upper_base,
            z - 0.08,
            BUILDING_X0 + 0.1,
            roof_y,
            z + 0.08,
            options,
        )
        out.box(
            BUILDING_X1 - 0.1,
            upper_base,
            z - 0.08,
            BUILDING_X1 + 0.1,
            roof_y,
            z + 0.08,
            options,
        )


def add_roof(out: MapWriter, roof_y: float, final_flight: int) -> None:
    out.section("Accessible roof reached by the final flight")
    add_floor_plate(out, roof_y, (STAIR_RUNS[final_flight % len(STAIR_RUNS)],))
    options = "tex=concrete_white top=metal_01 scale=60"
    parapet_height = 1.2
    out.box(
        BUILDING_X0,
        roof_y,
        BUILDING_Z0,
        BUILDING_X1,
        roof_y + parapet_height,
        BUILDING_Z0 + 0.3,
        options,
    )
    out.box(
        BUILDING_X0,
        roof_y,
        BUILDING_Z1 - 0.3,
        BUILDING_X1,
        roof_y + parapet_height,
        BUILDING_Z1,
        options,
    )
    out.box(
        BUILDING_X0,
        roof_y,
        BUILDING_Z0 + 0.3,
        BUILDING_X0 + 0.3,
        roof_y + parapet_height,
        BUILDING_Z1 - 0.3,
        options,
    )
    out.box(
        BUILDING_X1 - 0.3,
        roof_y,
        BUILDING_Z0 + 0.3,
        BUILDING_X1,
        roof_y + parapet_height,
        BUILDING_Z1 - 0.3,
        options,
    )


def generate(upper_floors: int) -> str:
    if upper_floors < 1:
        raise ValueError("upper_floors must be positive")

    out = MapWriter()
    building_height = LOBBY_HEIGHT + upper_floors * FLOOR_HEIGHT
    roof_y = GROUND_Y + building_height
    out.line("// Generated by scripts/gen_tremor_skyscraper.py.")
    out.line(
        f"// One lobby and {upper_floors} upper floors; "
        f"building height {number(building_height)} m."
    )
    out.line("// The site occupies x -50..50 and z -50..50, exactly 100 m by 100 m.")
    out.line("")
    out.line(f"spawn x=0 y={number(GROUND_Y + 1.6)} z=-30 yaw=0")

    add_lighting(out, upper_floors)
    add_site(out)
    add_lobby(out)

    out.section("Open upper floors, glass curtain wall, and right-side stairs")
    lower_y = GROUND_Y
    upper_y = GROUND_Y + LOBBY_HEIGHT
    add_stairs(out, lower_y, upper_y, flight=0)
    for floor in range(upper_floors):
        floor_y = GROUND_Y + LOBBY_HEIGHT + floor * FLOOR_HEIGHT
        ceiling_y = floor_y + FLOOR_HEIGHT
        out.line("")
        out.line(f"// Upper floor {floor + 1}")
        add_floor_plate(out, floor_y, (STAIR_RUNS[floor % len(STAIR_RUNS)],))
        add_perimeter_band(out, floor_y)
        add_glass_storey(out, floor_y, ceiling_y)
        add_stairs(out, floor_y, ceiling_y, flight=floor + 1)

    add_mullions(out, GROUND_Y + LOBBY_HEIGHT, roof_y)
    add_roof(out, roof_y, final_flight=upper_floors)
    return out.render()


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return parsed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--floors",
        type=positive_int,
        default=DEFAULT_UPPER_FLOORS,
        help="number of upper floors above the lobby (default: %(default)s)",
    )
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(generate(args.floors), encoding="utf-8")
    print(f"Wrote {args.output} with {args.floors} upper floors")


if __name__ == "__main__":
    main()
