export type HexColor = `#${string}`;

export type Anchor =
  | "top_left"
  | "top_center"
  | "top_right"
  | "center_left"
  | "center"
  | "center_right"
  | "bottom_left"
  | "bottom_center"
  | "bottom_right";

export type BlendMode =
  | "normal"
  | "multiply"
  | "screen"
  | "overlay"
  | "darken"
  | "lighten"
  | "color_dodge"
  | "color_burn"
  | "hard_light"
  | "soft_light"
  | "difference"
  | "exclusion";

export type FitMode = "fill" | "cover" | "contain";

export type Easing =
  | "linear"
  | "ease_in"
  | "ease_out"
  | "ease_in_out"
  | "ease_in_quad"
  | "ease_out_quad"
  | "ease_in_out_quad"
  | "ease_in_expo"
  | "ease_out_expo"
  | "ease_in_out_expo"
  | "ease_in_back"
  | "ease_out_back"
  | "ease_in_out_back"
  | "ease_out_bounce"
  | "cubic_bezier"
  | "spring";

export type AnimProperty =
  | "x"
  | "y"
  | "width"
  | "height"
  | "opacity"
  | "scale"
  | "rotation"
  | "skew_x"
  | "skew_y"
  | "cx"
  | "cy"
  | "r"
  | "radius"
  | "color"
  | "background"
  | "border_color"
  | "path_progress"
  | "stroke_dashoffset";

export interface GradientStop {
  offset: number;
  color: HexColor;
}

export interface GradientSpec {
  type?: "linear" | "radial";
  from?: HexColor;
  to?: HexColor;
  stops?: GradientStop[];
  angle?: number;
}

export interface ShadowSpec {
  color?: HexColor;
  offset_x?: number;
  offset_y?: number;
  blur?: number;
}

export interface SpringConfig {
  stiffness?: number;
  damping?: number;
  mass?: number;
}

export interface Keyframe {
  time: number;
  value: number | HexColor;
  easing?: Easing;
  bezier?: [number, number, number, number];
}

export interface Animation {
  property: AnimProperty;
  from?: number | HexColor;
  to?: number | HexColor;
  start_sec?: number;
  end_sec?: number;
  keyframes?: Keyframe[];
  easing?: Easing;
  bezier?: [number, number, number, number];
  spring?: SpringConfig;
  repeat?: number | "infinite";
  yoyo?: boolean;
}

export interface TextSpan {
  text: string;
  color?: HexColor;
  font_weight?: "normal" | "bold";
  font_family?: string;
  font_size?: number;
}

export interface TextOutline {
  color?: HexColor;
  width?: number;
}

export type DynamicText =
  | { mode: "typewriter"; text: string; start_sec?: number; end_sec?: number; cursor?: string }
  | {
      mode: "counter";
      from: number;
      to: number;
      decimals?: number;
      prefix?: string;
      suffix?: string;
      start_sec?: number;
      end_sec?: number;
      easing?: Easing;
    }
  | { mode: "timecode"; format?: "mm:ss" | "mm:ss.cc" | "hh:mm:ss"; offset_sec?: number };

export interface BaseLayer {
  name?: string;
  position?: "relative" | "absolute";
  anchor?: Anchor;
  skew_x?: number;
  skew_y?: number;
  start_sec?: number;
  end_sec?: number;
  duration_secs?: number;
  overlap_secs?: number;
  flex_grow?: number;
  margin?: number;
  blend_mode?: BlendMode;
  blur?: number;
  shadow?: ShadowSpec;
  animations?: Animation[];
}

export interface RectLayer extends BaseLayer {
  type: "rect";
  x?: number;
  y?: number;
  width: number;
  height: number;
  radius?: number;
  color: HexColor;
  gradient?: GradientSpec;
  border_color?: HexColor;
  border_width?: number;
  path_progress?: number;
  stroke_dashoffset?: number;
  opacity?: number;
  scale?: number;
  rotation?: number;
}

export interface CircleLayer extends BaseLayer {
  type: "circle";
  cx?: number;
  cy?: number;
  r: number;
  color: HexColor;
  border_color?: HexColor;
  border_width?: number;
  path_progress?: number;
  stroke_dashoffset?: number;
  opacity?: number;
  scale?: number;
}

export interface TextLayer extends BaseLayer {
  type: "text";
  x?: number;
  y?: number;
  content?: string;
  dynamic?: DynamicText;
  spans?: TextSpan[];
  font_size?: number;
  line_height?: number;
  letter_spacing?: number;
  font_family?: string;
  font_weight?: "normal" | "bold";
  text_align?: "left" | "center" | "right";
  color?: HexColor;
  gradient?: GradientSpec;
  outline?: TextOutline;
  opacity?: number;
  max_width?: number;
  scale?: number;
}

export interface SvgLayer extends BaseLayer {
  type: "svg";
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  src: string;
  path_progress?: number;
  stroke_dashoffset?: number;
  stroke_dasharray?: number[];
  opacity?: number;
  rotation?: number;
}

export interface ImageLayer extends BaseLayer {
  type: "image";
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  src: string;
  fit?: FitMode;
  opacity?: number;
  rotation?: number;
  scale?: number;
}

export interface VideoLayer extends BaseLayer {
  type: "video";
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  src: string;
  fit?: FitMode;
  trim_start_sec?: number;
  trim_end_sec?: number;
  playback_rate?: number;
  volume?: number;
  muted?: boolean;
  opacity?: number;
  rotation?: number;
  scale?: number;
}

export interface ContainerLayer extends BaseLayer {
  type: "container";
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  radius?: number;
  background?: HexColor;
  gradient?: GradientSpec;
  border_color?: HexColor;
  border_width?: number;
  opacity?: number;
  scale?: number;
  rotation?: number;
  clip?: boolean;
  cascade_opacity?: boolean;
  timeline?: "global" | "local" | "series";
  stagger_secs?: number;
  layout?: {
    direction?: "row" | "column";
    justify_content?: "start" | "center" | "end" | "space_between" | "space_around" | "stretch" | "flex_start" | "flex_end";
    align_items?: "start" | "center" | "end" | "space_between" | "space_around" | "stretch" | "flex_start" | "flex_end";
    gap?: number;
    padding?: number;
  };
  children?: Layer[];
}

export type Layer =
  | RectLayer
  | CircleLayer
  | TextLayer
  | SvgLayer
  | ImageLayer
  | VideoLayer
  | ContainerLayer;

export interface RustrenScene {
  schema_version?: "1.0" | "1.1";
  min_renderer_version?: string;
  canvas: {
    width: number;
    height: number;
    fps: number;
    duration_secs: number;
    background?: HexColor;
  };
  fonts?: string[];
  audio?: Array<{
    src: string;
    start_sec?: number;
    volume?: number;
    trim_start_sec?: number;
    trim_end_sec?: number;
    playback_rate?: number;
  }>;
  layers: Layer[];
}
