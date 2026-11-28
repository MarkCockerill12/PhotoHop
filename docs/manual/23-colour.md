# Colour adjustments

Step **4 Colour adjustments** changes brightness and colour by hand. A colour match (step 3) sets these same sliders for you; you can tweak them afterwards. Every slider is keyframable on video (with *Animate* on, see [Timing & animation](timing)).

## The everyday sliders

| Slider | What it does | Tip |
|---|---|---|
| Exposure | overall brightness in camera stops (+1 = twice as bright) | match how bright you look vs. things next to you |
| Temperature | left = cooler/bluer, right = warmer/more orange | sunset = warm, shade or night = cool |
| Tint | left = greener, right = more magenta | fixes green fluorescent or pink LED casts |
| Contrast | difference between darks and brights (1 = unchanged) | hazy or far-away scenes have less |
| Saturation | 0 = black and white, 1 = unchanged | game scenes are often less vivid than phone photos |
| Shadow level | raises or lowers the darkest parts (black level) | match the blackest blacks in the scene |

**On** turns all colour adjustments of this layer off and on to compare. **Reset all colour** goes back to the original colours (and forgets the colour match).

## Colour wheels (shadows / midtones / highlights)

Three wheels tint and brighten three ranges separately. Colourists call them **Lift** (shadows), **Gamma** (midtones) and **Gain** (highlights).

- Drag the dot towards a colour to tint that range. Double-click to reset.
- The number under each wheel brightens or darkens that range.
- **Contrast centre** (pivot) is the brightness that Contrast leaves unchanged.

Example: a scene with blue shadows and warm sunlight → drag the **Shadows** wheel slightly towards blue and the **Highlights** wheel slightly towards orange.

## Curves

Remap brightness with a curve: the horizontal axis is the input, the vertical the output. Bend the line **up** to brighten that range, **down** to darken it. Choose **Master** (all channels), **Red**, **Green** or **Blue** to change one colour channel.

- Click to add a point, drag to move it, double-click a point to delete it.
- **Reset curve** straightens it.

## Change one colour only

Selects a range of colours and changes only those (colourists call this a *qualifier*). Example: make a green shirt blue.

1. Tick **Enabled**.
2. Set **Hue** (which colour) and **Hue width** (how broad a range), **Sat low/high** and **Lum low/high** to narrow it down. **Softness** smooths the edge of the selection.
3. Tick **Show matte** to see what's selected (white = selected). Untick when done.
4. Change it with **Hue shift**, **Saturation** and **Brightness**. **Invert** changes everything except the selected colours.

## Checking colour with scopes

Your eyes adapt; scopes don't. Press **S** for the [Scopes](scopes) to compare your layer's black level and colour with the background's.
