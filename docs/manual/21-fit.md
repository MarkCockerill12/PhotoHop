# Fit into the scene

Step **3 Fit into the scene** makes a layer look like it belongs in the background. Colour is the biggest giveaway in any composite, followed by a missing shadow and a too-clean, too-sharp look. This section fixes all three.

## One-click fit

**✨ One-click fit** does three things in one go (one undo step each, all adjustable afterwards):

1. **Colour match** — the same as *Match colours* below.
2. **Shadow** — turns on a shadow on the ground starting at the layer's feet. If another layer in the project already has a shadow, it copies its direction, length and darkness (one sun for the whole scene) and scales the softness to this layer's size. Otherwise it aims the shadow away from the light it finds in the background.
3. **Grain** — copies the background's film/sensor noise.

It never adds blur. If you want the background's softness or motion blur too, use **Copy the camera's blur & grain** in [Camera & lens look](camera).

## Match colours (instant, no AI)

Measures the background **around** the layer and changes six ordinary colour sliders so the layer sits in that light:

| What it matches | How | Slider it sets |
|---|---|---|
| Colour of the light | compares only grey/white parts (a white shirt, grey metal) with grey parts of the scene | Temperature, Tint |
| Black level | the darkest parts should be as dark (or as hazy) as the scene's | Shadow level, Contrast |
| Brightness | highlights never brighter than the scene's; a little pull towards its mid-tones | Exposure, Contrast |
| Saturation | a gentle pull towards the scene's, so very vivid colours calm down | Saturation |

### Why your colours stay your colours

Matching only the light — not the object's own colours — is what lets PhotoHop match **any colour into any scene**. A deep-blue alien stays blue in a green-and-orange level; it just gets the scene's warm light, black level and saturation. If the layer has no grey or white parts at all (a fully blue figure), PhotoHop assumes it was lit with neutral light and only adds the scene's light colour.

This also removes coloured room lighting from photos of yourself: if you were lit by purple LEDs, your white or grey clothes show it, and the match replaces that purple with the scene's light.

## Match with AI

**Match with AI** asks an AI model (Harmonizer) how the layer should look in this scene. PhotoHop then finds the six slider values that reproduce that suggestion as closely as possible — so the result is always ordinary, editable sliders, never AI pixels. It is slower than *Match colours* and needs the [AI tools](ai) installed. Try both and keep the one you like (Ctrl+Z undoes).

## Match strength: Original ↔ Matched

After a match, a **Match strength** slider appears. Drag it to slide every changed setting between your **original** look (0%) and the **matched** look (100%). Watch the sliders in **4 Colour adjustments** move as you drag — they show exactly what the match changed.

- Tweak any colour slider after matching and the strength slider **keeps your tweak** at every strength.
- The line under the slider lists the changes, e.g. *Temperature +1.10, Shadow level +0.11*.
- **Forget the match** keeps the current sliders but removes the strength slider.

## Match to

Chooses where the colours to match come from:

- **The scene around this layer (best)** — the background in a band around the layer, plus a little of the whole picture.
- **All of "<layer>"** — the whole of another layer, e.g. a reference photo shot in the same place.

## When the match looks wrong

- **Layer must be above the background.** Use the arrows in the Layers list.
- **Leftover background in the cut-out** (a grey studio wall, green screen spill) confuses the match. Clean the cut-out first.
- **Several objects in one layer** standing in different places (one in sunlight, one in shade) get one compromise. Split them first: [Several objects in one picture](several-objects).
- Lower **Match strength** if the change is too strong, or adjust single sliders.
