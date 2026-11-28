# Layers

Everything in a PhotoHop project is a layer. Layers stack from bottom to top: the top of the Layers list is the front of the picture.

## Kinds of layer

| Icon | Kind | What it holds |
|---|---|---|
| picture | Photo | a still image (PNG, JPEG, WebP, TIFF, EXR) |
| film | Video | a video clip (MP4, MOV, MKV, WebM, AVI…) or an image sequence |
| brush | Paint | an empty layer you paint on with the Brush |
| square | Colour | one flat colour (useful with a blend mode as a tint) |

## Adding layers

- **Import**: drag files onto the window, use the picture button at the top of the Layers panel, or **File > Import (Ctrl+I)**. You can pick several files at once.
- **New paint layer** and **New colour layer**: the brush and square buttons at the top of the Layers panel, or the **Layer** menu.
- **Duplicate (Ctrl+D)** copies the selected layer, including its cut-out and settings.

The first picture you import into an empty project sets the canvas size; the first video also sets the frame rate and length.

## The Layers panel

- **Eye** — show or hide the layer.
- **Lock** — protects the layer from being moved on the canvas.
- **Name** — click to select. A **red** name means the layer's file could not be loaded (hover to see why), for example because it was moved.
- **Arrows** — move the layer up or down the stack. A layer must be **above** the background to be fitted into it.
- **Bin** (top) — delete the selected layer (**Del**).

## Renaming

Type a new name in the box at the top of the inspector.

## The background layer

The **bottom** layer is treated as the background. Its inspector shows only Position & size, Colour adjustments, Camera & lens look (for depth of field and bloom on the background), Game look and Replace a window or screen. The cut-out, fit and shadow steps appear on layers above it.

## Several objects in one picture

If one picture contains several separate things (three characters, two props), use **Split into separate objects** so each gets its own shadow and colour match. See [Several objects in one picture](several-objects).
