# Shadow & light

Step **5 Shadow & light** gives a layer a **shadow on the ground**, lets **shadows in the scene fall on it**, and can **relight** it from another direction. A missing or wrong shadow is the second most common giveaway after colour.

## Aim from the scene's light

**Aim from the scene's light** estimates where the light in the background comes from and its colour, then points the shadow away from it and sets the relight colour. It's a good start; real shadows already in the scene are a better guide (see below).

## Shadow on the ground

Tick **Shadow on the ground**. PhotoHop finds the feet (the lowest solid part of the cut-out) and throws the layer's silhouette onto the ground, sharp near the feet and softer further away, plus a short dark **contact** patch where the feet touch the ground.

| Setting | What it does |
|---|---|
| Direction | which way the shadow falls, degrees (0 = right, 90 = towards you, -90 = away) |
| Length | compared to the layer's height: low sun = long, midday = short |
| Thickness | widens a shadow that looks paper-thin when the light comes from the side (a cut-out is flat) |
| Softness | blur of the shadow edge in pixels: sunny = low, cloudy = high |
| Darkness | how dark the shadow is |
| Contact darkness | the small dark patch right under the feet |
| Feet line | where the layer touches the ground (fraction of its height, 1 = bottom edge). **Find feet** sets it |
| Shadow colour | shadows outdoors are slightly blue (lit by the sky) |

### Aiming the shadow

The easiest way: choose the **Move** tool (V). An **orange dot** shows where the shadow of the top of the layer lands. **Drag the dot** to where it should be. Look at shadows already in the background — a lamp post, a tree, a crate — and match their direction and length.

> All shadows from the same sun point the same way and have the same length relative to the height of what casts them. Copy a shadow that is already in the picture.

### Body-shaped shadow (AI)

A cut-out photo is flat, so a light from the side throws a thin, odd shadow. **Body-shaped shadow (AI)** rebuilds a person in 3D from the photo (SAM 3D Body) and projects the real body shape onto the ground. Tick **Use 3D body** to use it. It's for people; for other objects use **Thickness**.

## Scene shadows fall on me

If you stand where shadows fall — sun through leaves, window frames, blinds — those shadows should fall on you too. Tick **Scene shadows fall on me**: PhotoHop reads where the ground near you is sunlit or shaded and lays the same pattern over you, following the ground shadow's direction.

- Line up your **ground shadow** with the scene's shadows first (it can be invisible: set Darkness to 0).
- **Strength** — how dark the scene's shadows are on you. **Softness** — blur of the pattern. **Sun / shade split** — moves the line between sunlit and shaded ground.
- **My ground shadow only darkens sunlit ground** — your shadow doesn't double-darken places that are already in shade.

## Relight

Changes which side of the layer is lit, to match the scene's sun. It needs to know which way each part of the layer faces: click **Analyse shape (AI)** first (this makes a *normal map*; it's made for people).

- **Drag the sun** on the round pad: its direction is where the light comes from; the centre means straight from the camera, the edge means from the side.
- **Brightness** — strength of the light. **Fill light** — light on the side facing away (0 = black shadows). **Rim light** — a bright edge from behind. **Amount** — blend between your original lighting (0) and the new light (1).
- **Skin softness** — how softly light wraps around skin. **Light colour** and **Rim colour**.

Relight only changes shading; it never repaints your picture.

## Several objects

Each object needs its own shadow. See [Several objects in one picture](several-objects).
