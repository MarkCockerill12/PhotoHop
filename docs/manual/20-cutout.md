# Cut out

Step **1 Cut out** keeps the person or object and hides the rest of the layer's picture. In PhotoHop a cut-out is a **mask**: a black-and-white picture where white means *show* and black means *hide*. Masks are never destructive — your original picture is untouched and you can fix the mask at any time.

## Remove background (AI)

The big **Remove background (AI)** button finds the main subject and hides everything else. On a video layer it becomes **Remove background (AI, whole video)** and tracks the subject through every frame.

- Photos take a few seconds on a graphics card, up to a minute on a laptop CPU.
- It needs the AI tools installed. See [AI tools](ai#setting-up-the-ai-tools).

## Keep only… (describe it)

Opens a small window where you type what to keep — "person", "dog", "toy gun", "car on the left". AI selects just that. Useful when the photo has several things and you want one of them.

## Remove a colour

For plain backgrounds — a studio wall, a green screen, the flat grey behind a 3D render — remove one colour everywhere at once:

1. Click **Pick a colour to remove**.
2. Click the colour on the canvas (e.g. the background). Every pixel close to that colour disappears, anywhere in the picture — not only the area you clicked, so gaps between arms and legs go too. A small swatch shows the picked colour.
3. Adjust **Tolerance** (how different a colour may be and still be removed: raise it if bits of the background remain, lower it if parts of the object vanish) and **Softness** (a fade for smooth edges). The change updates live.

Ctrl+Z undoes. Pick again to remove a second colour. It combines with AI cut-outs, painting and pen shapes.

> Object contains the same colour as the background (a grey shirt on a grey wall)? Remove the colour, then bring the shirt back with the **Eraser** while *Paint on the cut-out* is ticked.

## Fix it by hand

After any cut-out:

- **Cut-out on** — untick to see the whole picture again (handy for checking).
- **Invert** — keep the outside instead (cut a hole).
- **Paint on the cut-out** — the **Brush** hides and the **Eraser** brings back. Use a soft brush (low hardness) for hair and fuzzy edges, a hard one for clothes and objects.
- **Remove the cut-out** — deletes the mask so the whole picture shows again.
- **Paint the cut-out** (when there is no cut-out yet) — starts an all-visible mask and switches to the brush, for cutting out completely by hand.
- **Select an area (M)** — use rectangle, ellipse, lasso or magic wand selections. See [Tools](tools#select-an-area-m).

> Zoom in (mouse wheel) when cleaning edges. Hide the background layer for a moment (its eye) to see leftovers against the empty canvas.

## Pen shapes

For exact outlines (a phone, a gun, a window frame), draw shapes with the **Pen (P)**. The list under **Pen shapes** shows each shape:

- Click a shape's name to edit it with the pen.
- The checkbox turns it on or off.
- **Add / Subtract**: Add keeps what's inside, Subtract cuts it away (e.g. the gap between an arm and the body).
- **Invert** flips that one shape.
- **◆** keyframes the shape at the playhead (for video: key it at a few frames and it moves smoothly between them).
- The bin deletes it.
- While editing a shape: **Feather** (soft edge, pixels) and **Opacity**.

Pen shapes combine with the AI or painted mask.

## Split into separate objects

If the picture holds several separate objects (three characters in one render), click **Split into separate objects**. See [Several objects in one picture](several-objects).

## Tips for a convincing cut-out

- Check the edges at 100% zoom. A thin line of the old background around you is the most common giveaway.
- Hair: keep it slightly soft rather than chopping it off.
- The bottom of the cut-out is treated as your **feet** for the shadow; trim anything below the feet (floor, reflection).
