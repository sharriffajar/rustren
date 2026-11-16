from __future__ import annotations

from typing import Any, Literal, TypedDict, Union

Anchor = Literal[
    "top_left",
    "top_center",
    "top_right",
    "center_left",
    "center",
    "center_right",
    "bottom_left",
    "bottom_center",
    "bottom_right",
]
FitMode = Literal["fill", "cover", "contain"]
Easing = Literal[
    "linear",
    "ease_in",
    "ease_out",
    "ease_in_out",
    "ease_in_quad",
    "ease_out_quad",
    "ease_in_out_quad",
    "ease_in_expo",
    "ease_out_expo",
    "ease_in_out_expo",
    "ease_in_back",
    "ease_out_back",
    "ease_in_out_back",
    "ease_out_bounce",
    "cubic_bezier",
    "spring",
]


class GradientStop(TypedDict):
    offset: float
    color: str


class GradientSpec(TypedDict, total=False):
    type: Literal["linear", "radial"]
    from_: str
    to: str
    stops: list[GradientStop]
    angle: float


class Keyframe(TypedDict, total=False):
    time: float
    value: Union[float, str]
    easing: Easing
    bezier: list[float]


class Animation(TypedDict, total=False):
    property: str
    from_: Union[float, str]
    to: Union[float, str]
    start_sec: float
    end_sec: float
    keyframes: list[Keyframe]
    easing: Easing
    bezier: list[float]
    spring: dict[str, float]
    repeat: Union[int, Literal["infinite"]]
    yoyo: bool


class TextSpan(TypedDict, total=False):
    text: str
    color: str
    font_weight: Literal["normal", "bold"]
    font_family: str
    font_size: float


class TextOutline(TypedDict, total=False):
    color: str
    width: float


class Canvas(TypedDict, total=False):
    width: int
    height: int
    fps: int
    duration_secs: float
    background: str


class RustrenScene(TypedDict, total=False):
    schema_version: Literal["1.0", "1.1"]
    min_renderer_version: str
    canvas: Canvas
    fonts: list[str]
    audio: list[dict[str, Any]]
    layers: list[dict[str, Any]]
