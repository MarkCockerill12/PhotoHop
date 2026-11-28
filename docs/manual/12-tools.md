# Tools

Tools live in the toolbar. Hover a tool to see its name; press its key to switch.

| Key | Tool | What it does |
|---|---|---|
| V | Move | drag, resize and rotate layers; drag the orange shadow dot |
| M | Select an area | rectangle, ellipse, lasso or magic wand; the selection becomes the layer's cut-out |
| C | Crop the canvas | drag the area to keep, then Enter (Esc cancels) |
| B | Brush | paint on paint layers, or hide parts of a cut-out |
| E | Eraser | erase paint, or bring back parts of a cut-out |
| P | Pen | draw exact cut-out shapes with curves |
| H | Hand | pan the view (also: right or middle mouse drag, touchpad right-click hold and drag, or Space-drag) |

## Move (V)

See [Position & size](position). With a shadow turned on, an **orange dot** shows where the shadow of the top of the layer lands; drag it to aim the shadow. See [Shadow & light](shadow#aiming-the-shadow).

## Select an area (M)

Choose the shape in the toolbar: **Rectangle**, **Ellipse**, **Lasso** (drag around) or **Magic wand** (click a colour; **Tolerance** sets how similar colours must be).

- A plain selection **replaces** the cut-out with what you selected.
- Hold **Shift** to **add** to it, **Alt** to **subtract** from it.
- **Feather** softens the selection's edge (in pixels).

The selection always goes into the selected layer's cut-out (its mask).

## Crop the canvas (C)

Drag a rectangle on the canvas, then press **Enter** to crop the whole project to it (all layers keep their place). **Esc** cancels.

## Brush (B) and Eraser (E)

Options in the toolbar: **Size** (pixels), **Hardness** (soft to hard edge), **Opacity** and, for the brush, the **colour**.

- On a **paint layer**, the brush paints and the eraser erases.
- On a layer with a cut-out and **Paint on the cut-out** ticked (in [Cut out](cutout)), the brush **hides** and the eraser **brings back**. The toolbar shows *Editing mask* in yellow while this is on.

## Pen (P)

Draws bezier shapes that become part of the cut-out:

- **Click** to add a point; **drag** while adding to pull out a smooth curve.
- **Click the first point** or press **Enter** to close the shape.
- Drag points and handles to adjust; **Alt-drag** a handle to break the curve into a corner.
- **Del** removes the point under the mouse; **Esc** finishes.

Each shape can **Add** (keep inside) or **Subtract** (cut away), be inverted, feathered and faded. On video, press the shape's ◆ at several frames and it follows by moving between them. See [Cut out](cutout#pen-shapes).

## Hand (H)

Drag to pan. With any tool you can also pan by dragging with the right or middle mouse button (touchpad: right-click, hold and drag) or holding Space while dragging. Scroll to zoom.
