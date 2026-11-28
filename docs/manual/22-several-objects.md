# Several objects in one picture

Sometimes one picture holds several separate things — for example a render of **three characters**: two in the foreground and one far away on a hill. They stand in different places, so each needs its **own** shadow (different feet, different size, different softness) and its **own** colour match (different surroundings). One layer can only have one of each, so split the picture first.

## Split into separate objects

1. Cut out the picture as usual (see [Cut out](cutout)) so only the objects are visible.
2. Select the layer and click **Split into separate objects** in step **1 Cut out** (or **Layer > Split into separate objects**).

PhotoHop finds every separate object (anything with a gap around it), and for each one:

- creates a new layer named *<name> 1*, *<name> 2*, … from left to right, cropped tightly around that object and placed exactly where it was;
- copies the original layer's colour and effect settings;
- finds that object's **feet** (the lowest solid part) for its shadow.

The original layer is kept, **hidden**, and renamed *(original)* — show it again (eye button) if you need it. Undo (Ctrl+Z) puts everything back.

Pieces that are close together (a gap in an arm, a weapon held in a hand) stay with their object. Tiny specks are ignored.

## Then fit each object

Select each new layer in turn and click **✨ One-click fit**:

- Each gets a colour match from **its own** surroundings — the figure on the green hill matches the hill, the ones on the platform match the platform.
- Each gets a shadow from **its own** feet, with the same sun direction as the other shadows in the scene and softness scaled to its size, so a small far-away figure gets a small, tighter shadow.

## Fine-tuning per object

- **Shadow direction**: with the Move tool, drag each object's orange dot. Copy the direction of a real shadow in the background. All objects lit by the same sun should point the same way.
- **Far away objects** usually need **less contrast**, a little **Haze** (in [Camera & lens look](camera)) and softer, fainter shadows.
- **Ground that slopes** (a hill): lower the shadow's **Length** so it doesn't climb the slope, and adjust **Feet line** if the shadow starts too high or low.

## Doing it by hand instead

You can also **Duplicate (Ctrl+D)** the layer once per object and, on each copy, use the Pen or Brush to hide the other objects. Splitting is faster and keeps each layer small.
