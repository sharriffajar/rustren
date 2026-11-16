# Rustren Scene Schema (v1.1) — AI & Developer Guide

`rustren` adalah browserless 2D video render engine. Semua video didefinisikan dengan **1 file JSON deklaratif** tanpa HTML/CSS/JS.

> Gunakan `rustren capabilities` untuk memeriksa fitur dan versi renderer (`renderer_version: "1.1"`, `schema_version: "1.1"`, `min_schema_version: "1.0"`) sebelum menghasilkan JSON.

## Aturan Penting (One-Shot Rules)
1. **Jangan gunakan field di luar skema ini** (lihat [schema.json](schema.json) untuk JSON Schema resmi).
2. **Timing Scene**: Gunakan `start_sec`, `end_sec`, atau `duration_secs` (dalam detik), serta `timeline: "series"` pada container untuk sequencing otomatis.
3. **Animasi `x` / `y`**:
   - Pada root layer (`position: "absolute"`): `from` dan `to` adalah **koordinat kanvas absolut**.
   - Pada child `relative` di dalam `container`: hindari menganimasikan `x`/`y` jika tidak perlu; gunakan `opacity` dan `scale` agar posisi `taffy` tetap stabil.
4. **Animasi `width` / `height` di dalam `container` (mis. progress bar)**: Gunakan `position: "absolute"`, `x: 0`, `y: 0` pada `rect` tersebut agar lebarnya tidak tertahan oleh kalkulasi `taffy`.
5. **Background Grid & Radial Glow**: Gunakan layer `type: "svg"` dengan `<pattern>` dan `<radialGradient>` di urutan paling bawah `layers`.
6. **Teks Multi-baris**: Selalu isi `max_width` pada `type: "text"` agar word-wrapping dan estimasi tinggi `taffy` akurat.
7. **Cache**: Baca tabel [Cache Engine](#cache-engine-key--cara-nulis-json-biar-hit) di bawah. Taruh layer background statis di indeks paling awal `layers[]`, dan utamakan animasi `opacity` / `x` / `y` / `rotation` biar **HIT** (hindari animasi `width` SVG / `path_progress` / `scale` teks kalau bukan perlu).

---

## Struktur Root
```json
{
  "schema_version": "1.1",
  "min_renderer_version": "1.0",
  "canvas": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "duration_secs": 15.0,
    "background": "#070B14"
  },
  "fonts": ["/path/to/Inter-Bold.ttf"],
  "audio": [
    {
      "src": "/path/to/audio.mp3",
      "start_sec": 0.0,
      "volume": 0.8,
      "trim_start_sec": 0.0,
      "trim_end_sec": 15.0,
      "playback_rate": 1.0
    }
  ],
  "layers": [ ... ]
}
```

## Properti Umum Setiap Layer
- `name` (opsional): string
- `type` (wajib): `"container"` | `"rect"` | `"circle"` | `"text"` | `"svg"` | `"image"` | `"video"`
- `position` (opsional, default `"relative"`): `"relative"` | `"absolute"`
- `start_sec` (opsional, default `0.0`): f32
- `end_sec` (opsional, default `canvas.duration_secs`): f32
- `duration_secs` (opsional): f32 (alternatif `end_sec = start_sec + duration_secs`, wajib untuk sequencing `series`)
- `overlap_secs` (opsional, default `0.0`): f32 (overlap mundur antar item di mode container `timeline: "series"`)
- `flex_grow` (opsional): f32
- `margin` (opsional): f32
- `blend_mode` (opsional, default `"normal"`): `"normal"` | `"multiply"` | `"screen"` | `"overlay"` | `"darken"` | `"lighten"` | `"difference"`
- `blur` (opsional, default `0.0`): f32 (Gaussian blur radius dalam px)
- `shadow` (opsional): `{ "offset_x": 0.0, "offset_y": 8.0, "blur": 16.0, "color": "#00000080" }`
- `animations` (opsional): array of Animation

## Tipe Layer

### 1. `container` (Flexbox via `taffy` & Nested Timeline)
- `x`, `y` (default `0.0`), `width`, `height` (opsional, auto-size jika kosong)
- `radius` (default `0.0`), `background` (opsional hex `#RGB` / `#RRGGBB` / `#RRGGBBAA`)
- `gradient` (opsional): `{ "from": "#22D3EE", "to": "#818CF8", "angle": 90 }`
- `border_color` (opsional), `border_width` (default `0.0`), `opacity` (default `1.0`), `scale` (default `1.0`), `rotation` (default `0.0`, derajat), `clip` (default `false`), `cascade_opacity` (default `false`)
- `timeline` (opsional, default `"absolute"`): `"absolute"` | `"local"` (waktu animasi child relatif terhadap `start_sec` container) | `"series"` (children dirangkai berurutan otomatis berdasarkan `duration_secs` & `overlap_secs`)
- `layout`:
  - `direction`: `"row"` | `"column"`
  - `justify_content`: `"start"` | `"center"` | `"end"` | `"space_between"` | `"space_around"` | `"stretch"` (alias: `"flex_start"`, `"flex_end"`)
  - `align_items`: `"start"` | `"center"` | `"end"` | `"space_between"` | `"space_around"` | `"stretch"` (alias: `"flex_start"`, `"flex_end"`)
  - `gap` (default `0.0`), `padding` (default `0.0`)
- `children`: array of Layer

### 2. `rect`
- `x`, `y` (default `0.0`), `width`, `height` (wajib)
- `radius` (default `0.0`), `color` (wajib hex; jika `gradient` juga diisi, **`gradient` yang menang**), `scale` (default `1.0`), `rotation` (default `0.0`, derajat)
- `gradient`, `border_color`, `border_width`, `path_progress` (default `1.0`), `stroke_dashoffset` (opsional), `opacity`

### 3. `circle`
- `cx`, `cy` (default `0.0`), `r` (wajib), `color` (wajib hex), `border_color` (opsional), `border_width` (default `0.0`), `path_progress` (default `1.0`), `stroke_dashoffset` (opsional), `opacity` (default `1.0`), `scale` (default `1.0`)
- Jika diletakkan sebagai child `relative` di dalam `container`, ukurannya di layout flex adalah `2r × 2r` dan `cx`/`cy` ditimpa ke titik tengah kotak flex.

### 4. `text`
- `x`, `y` (default `0.0`), `content` (opsional jika `spans` diisi)
- `spans` (opsional): array of `{ "text": "...", "color": "#...", "font_weight": "bold", "font_family": "...", "font_size": 48.0 }` untuk rich text multi-style
- `font_size` (default `48.0`), `line_height` (opsional, default `1.25 × font_size`), `letter_spacing` (default `0.0`, px), `font_family` (opsional), `font_weight`: `"normal"` | `"bold"`
- `text_align`: `"left"` | `"center"` | `"right"`
- `color` (default `"#FFFFFF"`), `opacity` (default `1.0`), `scale` (default `1.0`), `max_width` (opsional, disarankan diisi)

### 5. `svg`
- `x`, `y` (default `0.0`), `width`, `height` (opsional), `src` (wajib: path file, URL, atau inline `<svg ...>...</svg>`), `path_progress` (default `1.0`), `stroke_dashoffset` (opsional), `stroke_dasharray` (opsional array of f32), `opacity` (default `1.0`), `rotation` (default `0.0`, derajat)

### 6. `image`
- `x`, `y` (default `0.0`), `width`, `height` (opsional), `src` (wajib: path file atau URL), `opacity` (default `1.0`), `rotation` (default `0.0`, derajat), `scale` (default `1.0`)

### 7. `video`
- `x`, `y` (default `0.0`), `width`, `height` (opsional, default ukuran asli video), `src` (wajib: path file atau URL video MP4/MOV/WEBM)
- `trim_start_sec` (default `0.0`), `trim_end_sec` (opsional), `playback_rate` (default `1.0`, mempengaruhi video & audio), `volume` (default `1.0`), `muted` (default `false`)
- `opacity` (default `1.0`), `rotation` (default `0.0`), `scale` (default `1.0`)

## `animations[]`
```json
{
  "property": "opacity",
  "from": 0.0,
  "to": 1.0,
  "start_sec": 0.0,
  "end_sec": 0.5,
  "easing": "spring"
}
```
- `property`: `"x"` | `"y"` | `"width"` | `"height"` | `"opacity"` | `"scale"` | `"rotation"` | `"cx"` | `"cy"` | `"r"` | `"radius"` | `"color"` *(alias: `"background"`)* | `"border_color"` | `"path_progress"` | `"stroke_dashoffset"`
- `from`, `to`: angka (`f32`) atau hex color (`"#RRGGBB"` / `"#RRGGBBAA"`) untuk `color` / `background` / `border_color`
- `easing`: `"linear"` | `"ease_in"` | `"ease_out"` | `"ease_in_out"` | `"ease_in_quad"` | `"ease_out_quad"` | `"ease_in_out_quad"` | `"ease_in_expo"` | `"ease_out_expo"` | `"ease_in_out_expo"` | `"ease_in_back"` | `"ease_out_back"` | `"ease_in_out_back"` | `"ease_out_bounce"` | `"cubic_bezier"` | `"spring"`
- `bezier` *(opsional, khusus `easing: "cubic_bezier"`)*: `[x1, y1, x2, y2]`
- `spring` *(opsional, khusus `easing: "spring"`)*: `{ "stiffness": 120.0, "damping": 14.0, "mass": 1.0 }`
- `repeat` *(opsional)*: angka integer (`>= 0`) atau `"infinite"` untuk mengulang siklus `[start_sec, end_sec]`
- `yoyo` *(opsional, default `false`)*: jika `true`, siklus ganjil membalik arah (`to` → `from`)


## Semantik Waktu, Animasi & Pewarisan
- **Waktu animasi (`animations[].start_sec` / `end_sec`)**: Bersifat **global** (mengikuti timeline video, bukan relatif ke `start_sec` layer).
- **Clamping & Multi-keyframe**: Sebelum animasi pertama dimulai, properti dikunci ke `from`. Di antara dua animasi berurutan pada properti yang sama (misal fade-in `0→1` lalu fade-out `1→0`), nilai ditahan di `to` animasi sebelumnya. Sesudah animasi terakhir selesai, properti dikunci ke `to`. Pola `opacity: 0` + fade-in + fade-out di satu layer aman digunakan.
- **`end_sec` layer**: Bersifat **inklusif** (`time_sec <= end_sec` tetap dirender), dikuantisasi per frame (`frame_idx / fps`). Untuk hard cut tanpa 1 frame tumpang tindih, gunakan `end_sec = start_berikutnya - 1/fps`.
- **Z-order**: Urutan elemen di `layers[]` dan `children[]` adalah urutan gambar dari belakang ke depan (elemen pertama di bawah, elemen terakhir di atas).
- **Pewarisan container**:
  - `opacity` container **tidak** cascade ke `children` secara default (`cascade_opacity: false`), namun jika `opacity <= 0` maka seluruh `children` ikut tidak dirender. Aktifkan `"cascade_opacity": true` pada `container` untuk fade-in/out satu scene sekaligus (jika ada container bersarang, aktifkan `cascade_opacity: true` juga di inner container agar opacity efektifnya terus mengalir ke cucu).
  - `start_sec` / `end_sec` pada `children` tetap dievaluasi sesuai mode `timeline` container (`"absolute"`, `"local"`, atau `"series"`), dan jika parent container berada di luar rentang aktifnya, seluruh `children` ikut tidak dirender.
  - `cx`, `cy`, `r` berlaku pada `circle` (memungkinkan animasi pulse & ripple).
  - `rotation` berlaku pada `container`, `rect`, `image`, `video`, dan `svg` (diabaikan pada `text` dan `circle`).
  - `scale` berlaku pada `container`, `rect`, `text`, `image`, `video`, dan `circle`.
- **Easing `spring`, Clamping & Error Handling**:
  - `spring` dapat sedikit overshoot melewati `to` sebelum stabil. Hasil animasi `opacity` selalu di-clamp engine ke `[0.0, 1.0]`, dan elemen dengan `width <= 0`, `height <= 0`, `r <= 0`, `scale <= 0`, atau `opacity <= 0` tidak digambar. Jika dua animasi pada properti yang sama saling tumpang tindih rentang waktunya, animasi yang muncul terakhir di array menang.
  - Jika file/URL `src` (`image`, `video`, `svg`, `audio`, `fonts`) tidak ditemukan atau gagal dimuat, render langsung berhenti dengan error (exit non-zero). Jika `font_family` tidak ditemukan, teks fallback ke sans-serif sistem.

## Aturan Ukuran, Layout & Teks
- **`container`**: `width` dan `height` **opsional** (default `0`, auto-size mengikuti isi jika kosong). Jika `position: "absolute"` dipakai pada child di dalam `container`, koordinat `x`/`y` bersifat **relatif terhadap `container`**, bukan kanvas.
- **Teks**:
  - Jika `fonts: []`, engine otomatis fallback ke font sans-serif sistem.
  - `font_family` dicocokkan dengan nama internal font (family name). `font_weight: "bold"` meminta weight bold dari `cosmic-text` (fallback ke sintetis/regular jika tidak tersedia).
  - `text_align` meratakan teks di dalam kotak `max_width` (atau lebar kanvas jika `max_width` kosong). **Catatan**: untuk `text` di dalam `container`, isi `max_width` agar teks tidak melebar keluar container. Mendukung `\n` (newline) dan emoji sistem.
- **Gradien & Warna**: Format warna hanya mendukung hex (`#RGB`, `#RRGGBB`, `#RRGGBBAA`). Pada `gradient`, `angle` dalam derajat diukur searah jarum jam dari sumbu X positif (`0°` = kiri ke kanan, `90°` = atas ke bawah).
- **SVG**: Dirender oleh `resvg`/`usvg` (mendukung `<path>`, `<rect>`, `<circle>`, `<pattern>`, `<radialGradient>`, `<linearGradient>`, dll.).

## Spesifikasi Output & Performa
- **Format**: MP4 (H.264 High Profile, `yuv420p`, `30fps` default).
- **Ukuran & Kecepatan**: Desain flat/vektor 1080×1920 berdurasi ~32s menghasilkan ~1,06 MB (~260 kbps) dengan kecepatan render **~12–16 fps** (estimasi kasar: `total_frames / 14` detik).

## Panduan Ukuran & Tipografi Aman (9:16 / 1080×1920)
- **Margin aman**: ≥ `60` px di kiri/kanan, ≥ `120` px di atas/bawah (menghindari UI TikTok/Reels).
- **Tipografi**:
  - Judul utama: `font_size` ≥ `64`
  - Subjudul / judul kartu: `font_size` ≥ `44`
  - Body text (isi di dalam kartu/paragraf): `font_size` ≥ `40` (hindari `< 34` karena sulit dibaca di layar HP)
  - Label / badge kecil: `font_size` ≥ `26`

## Sistem Koordinat, Anchor & Validasi
- **Sistem koordinat**: Origin `(0, 0)` di **pojok kiri-atas**, sumbu `x` ke kanan, sumbu `y` ke bawah, satuan **px**. Root layer selalu berada di koordinat absolut kanvas.
- **Anchor & Pivot**: `rect`, `container`, `image`, `svg`, dan `text` (`y` = sisi atas kotak teks, `line_height ≈ 1.25 × font_size` default) berpatokan pada pojok kiri-atas. `circle` berpusat dan berpivot `scale` di `(cx, cy)`. `image` berpivot `scale`/`rotation` dan `rect`/`svg` berpivot `rotation` di `(x + w/2, y + h/2)` pada frame tersebut (jika `width`/`height` ikut dianimasikan, titik tengahnya ikut bergeser). `text` men-scale `font_size` dan `line_height` dari pojok kiri-atas kotak teksnya.
- **Validasi CLI**: Jalankan `rustren validate <file.json>` sebelum render. Jika valid, keluar dengan exit code `0`, ringkasan ke stdout, dan lint `warning: ...` ke stderr (`end_sec > duration_secs`, `font_size < 26`, atau `text` di `container` tanpa `max_width`). Jika JSON tidak valid, keluar dengan exit code non-zero dan pesan error.

## Contoh JSON Lengkap (1 Scene + Progress Bar)
```json
{
  "canvas": { "width": 1080, "height": 1920, "fps": 30, "duration_secs": 5.0, "background": "#070B14" },
  "fonts": [],
  "audio": [],
  "layers": [
    {
      "type": "container",
      "name": "scene_1",
      "x": 60,
      "y": 300,
      "width": 960,
      "opacity": 0.0,
      "cascade_opacity": true,
      "start_sec": 0.0,
      "end_sec": 4.9667,
      "layout": { "direction": "column", "gap": 28, "padding": 40 },
      "background": "#111A2E",
      "radius": 24,
      "animations": [
        { "property": "opacity", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 0.4, "easing": "ease_out" },
        { "property": "opacity", "from": 1.0, "to": 0.0, "start_sec": 4.5, "end_sec": 4.9, "easing": "ease_in" }
      ],
      "children": [
        { "type": "text", "content": "JUDUL UTAMA", "font_size": 72, "font_weight": "bold", "color": "#22D3EE", "max_width": 880 },
        { "type": "text", "content": "Penjelasan singkat yang jelas dibaca di layar HP.", "font_size": 44, "color": "#FFFFFF", "max_width": 880 }
      ]
    },
    {
      "type": "rect",
      "name": "progress",
      "x": 60,
      "y": 1790,
      "width": 0,
      "height": 8,
      "color": "#22D3EE",
      "radius": 4,
      "animations": [
        { "property": "width", "from": 0.0, "to": 960.0, "start_sec": 0.0, "end_sec": 5.0, "easing": "linear" }
      ]
    }
  ]
}
```

## Resep Pola Layout Cepat
- **Kartu Perbandingan 2 Kolom (`row` + `flex_grow`)**:
```json
{
  "type": "container",
  "x": 60,
  "y": 800,
  "width": 960,
  "layout": { "direction": "row", "gap": 24 },
  "children": [
    {
      "type": "container",
      "flex_grow": 1,
      "background": "#111A2E",
      "radius": 24,
      "layout": { "direction": "column", "gap": 12, "padding": 32 },
      "children": [
        { "type": "text", "content": "#7", "font_size": 76, "font_weight": "bold", "color": "#22D3EE", "text_align": "center", "max_width": 400 },
        { "type": "text", "content": "PDB PPP", "font_size": 36, "color": "#94A3B8", "text_align": "center", "max_width": 400 }
      ]
    },
    {
      "type": "container",
      "flex_grow": 1,
      "background": "#111A2E",
      "radius": 24,
      "layout": { "direction": "column", "gap": 12, "padding": 32 },
      "children": [
        { "type": "text", "content": "#1", "font_size": 76, "font_weight": "bold", "color": "#818CF8", "text_align": "center", "max_width": 400 },
        { "type": "text", "content": "Terbesar ASEAN", "font_size": 36, "color": "#94A3B8", "text_align": "center", "max_width": 400 }
      ]
    }
  ]
}
```
- **Daftar Poin (`column` + aksen indikator)**:
```json
{
  "type": "container",
  "x": 60,
  "y": 600,
  "width": 960,
  "background": "#111A2E",
  "radius": 24,
  "layout": { "direction": "column", "gap": 20, "padding": 36 },
  "children": [
    { "type": "text", "content": "• Poin pertama yang ringkas", "font_size": 42, "color": "#FFFFFF", "max_width": 888 },
    { "type": "text", "content": "• Poin kedua dengan warna aksen", "font_size": 42, "color": "#22D3EE", "max_width": 888 }
  ]
}
```

---

## Resep Fitur v1.1 (Copy-Paste Ready)

### 1. SVG Path Draw-On (`path_progress`)
Gunakan `path_progress` (`0.0` → `1.0`) langsung di layer `svg`—engine otomatis menghitung panjang path sebenarnya (tidak perlu menebak `stroke_dasharray` manual). Gradient fill di dalam SVG tetap ter-render selama animasi berlangsung.
```json
{
  "type": "svg",
  "x": 44,
  "y": 230,
  "width": 612,
  "height": 150,
  "path_progress": 0.0,
  "src": "<svg viewBox='0 0 612 150' xmlns='http://www.w3.org/2000/svg'><path d='M0 130 Q 150 40, 300 95 T 612 42' fill='none' stroke='#22D3EE' stroke-width='5' stroke-linecap='round'/></svg>",
  "animations": [
    { "property": "path_progress", "from": 0.0, "to": 1.0, "start_sec": 0.5, "end_sec": 1.5, "easing": "ease_in_out_expo" }
  ]
}
```

### 2. Morphing Container (`radius`, `background`, `clip`)
Satu `container` dengan `"clip": true` bisa ber-morph mulus dari tombol pill menjadi kartu penuh sambil mengganti isi `children` berdasarkan window `start_sec`/`end_sec`:
```json
{
  "type": "container",
  "x": 790,
  "y": 496,
  "width": 340,
  "height": 88,
  "radius": 44,
  "background": "#141418",
  "clip": true,
  "shadow": { "offset_x": 0, "offset_y": 24, "blur": 48, "color": "#00000045" },
  "animations": [
    { "property": "x", "from": 790.0, "to": 650.0, "start_sec": 0.5, "end_sec": 0.8, "easing": "spring", "spring": { "stiffness": 210, "damping": 20, "mass": 1 } },
    { "property": "width", "from": 340.0, "to": 620.0, "start_sec": 0.5, "end_sec": 0.8, "easing": "spring", "spring": { "stiffness": 210, "damping": 20, "mass": 1 } },
    { "property": "radius", "from": 44.0, "to": 32.0, "start_sec": 0.5, "end_sec": 0.8, "easing": "ease_out" },
    { "property": "background", "from": "#141418", "to": "#FFFFFF", "start_sec": 0.5, "end_sec": 0.8, "easing": "ease_out" }
  ],
  "children": []
}
```

### 3. Color Interpolation & `cubic_bezier`
Animasikan `color` / `background` / `border_color` dengan hex string pada `from` dan `to`:
```json
{
  "type": "text",
  "x": 72,
  "y": 28,
  "content": "Week",
  "font_size": 28,
  "font_weight": "bold",
  "color": "#64748B",
  "max_width": 120,
  "animations": [
    {
      "property": "color",
      "from": "#64748B",
      "to": "#FFFFFF",
      "start_sec": 1.0,
      "end_sec": 1.25,
      "easing": "cubic_bezier",
      "bezier": [0.22, 1.0, 0.36, 1.0]
    }
  ]
}
```

### 4. Rich Text (`spans`), `letter_spacing` & Color Emoji
- Gunakan `spans` untuk mencampur warna dan ketebalan dalam satu paragraf.
- Untuk teks emoji (`😊`, `🔥`, `❤️`), **kosongkan `font_family`** agar engine otomatis mengarahkan ke font color emoji sistem (`Noto Color Emoji` / `Apple Color Emoji`).
```json
{
  "type": "text",
  "x": 80,
  "y": 120,
  "font_size": 48,
  "letter_spacing": 1.5,
  "max_width": 900,
  "spans": [
    { "text": "Render ", "color": "#FFFFFF" },
    { "text": "100% deterministik ", "color": "#22D3EE", "font_weight": "bold" },
    { "text": "tanpa browser.", "color": "#94A3B8" }
  ]
}
```

### 5. Relative Sequencing (`timeline: "series"` & `"local"`)
- `timeline: "series"`: tiap child otomatis berjalan berurutan sesuai `duration_secs` (dan mundur sebesar `overlap_secs` untuk crossfade), dengan waktu animasi child dimulai dari `0.0` (lokal terhadap masing-masing child).
```json
{
  "type": "container",
  "timeline": "series",
  "x": 0,
  "y": 0,
  "width": 1080,
  "height": 1920,
  "children": [
    {
      "type": "container",
      "duration_secs": 3.0,
      "background": "#0F172A",
      "children": [
        {
          "type": "text",
          "content": "Slide 1",
          "max_width": 800,
          "animations": [{ "property": "opacity", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 0.4 }]
        }
      ]
    },
    {
      "type": "container",
      "duration_secs": 3.0,
      "overlap_secs": 0.3,
      "background": "#1E293B",
      "children": [
        {
          "type": "text",
          "content": "Slide 2 (crossfade 0.3s)",
          "max_width": 800,
          "animations": [{ "property": "opacity", "from": 0.0, "to": 1.0, "start_sec": 0.0, "end_sec": 0.3 }]
        }
      ]
    }
  ]
}
```

### 6. Video Clip Layer & Visual Effects (`video`, `blur`, `blend_mode`)
```json
{
  "type": "video",
  "x": 0,
  "y": 0,
  "width": 1920,
  "height": 1080,
  "src": "/path/to/clip.mp4",
  "trim_start_sec": 2.0,
  "trim_end_sec": 10.0,
  "playback_rate": 1.25,
  "volume": 0.8,
  "muted": false
}
```
```json
{
  "type": "circle",
  "cx": 1500,
  "cy": 280,
  "r": 240,
  "color": "#22D3EE",
  "opacity": 0.25,
  "blur": 60.0,
  "blend_mode": "screen"
}
```

---

## Cache Engine: Key & Cara Nulis JSON Biar Hit

| Cache | Scope & Key | Cara Nulis JSON Biar Hit (dan Penyebab Miss) |
| :--- | :--- | :--- |
| **Static Background Snapshot** | Prefix `layers[0..k]` per scene | **Hit**: Taruh semua layer background 100% statis (`start_sec: 0`, `end_sec` kosong/`>= duration_secs`, `animations: []`, bukan `video`) di **indeks paling awal `layers[]`**. **Miss**: Menaruh layer beranimasi di indeks `0` menghentikan scan prefix untuk layer statis di bawahnya. |
| **Blurred-Layer Cache** (`THREAD_BLUR_CACHE`) | `(fnv1a(kind_tanpa_opacity, blur), canvas_w, canvas_h)` per thread | **Hit**: Pakai `blur > 0` tanpa `shadow` pada layer root non-`container`/non-`video`, dan hanya animasikan `opacity` (atau statis). **Miss**: Menambahkan `shadow`, menaruh di dalam `container`, atau menganimasikan `x`/`y`/`r`/`width`/`height`. |
| **SVG Raster Cache** (`THREAD_SVG_RASTER_CACHE`) | `(fnv1a(svg_bytes), byte_len, pw, ph)` per thread | **Hit**: Pakai `svg` tanpa `path_progress`/`stroke_dashoffset`/`stroke_dasharray`, dan jangan animasikan `width`/`height`. Animasi `x`, `y`, `rotation`, dan `opacity` **tetap hit**! |
| **SVG Tree Cache** (`THREAD_SVG_CACHE`) | `(fnv1a(svg_bytes), byte_len)` per thread | **Hit**: Inline `<svg>` atau `src` yang sama otomatis di-parse sekali per thread (berguna saat `path_progress` dianimasikan). |
| **AssetStore & Disk Cache** | Disk: `SHA-256(url)` · RAM: `src` string | **Hit**: Gunakan string `src` yang persis sama jika gambar/SVG/video dipakai ulang di beberapa layer agar tidak di-decode ganda. |
| **Font & Glyph Cache** (`THREAD_FONT_SYS`) | `(fonts.len(), glyph_cache_key)` per thread | **Hit**: Animasi `opacity`/`x`/`y` teks selalu hit cache glyph. **Miss**: Menganimasikan `scale` teks memicu rasterisasi glyph baru di tiap ukuran frame. |

### Aturan Koordinat Tambahan
- **Koordinat `circle` di dalam `container`**: Saat memakai `position: "absolute"` pada `circle` di dalam `container`, `cx`/`cy` di `animations` tetap bersifat **relatif terhadap pojok kiri-atas `container`**, bukan kanvas root.
