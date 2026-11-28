# Camera & lens look

Step **6 Camera & lens look** makes a layer look shot with the same camera as the background: the same grain, softness, motion blur, haze and glow. A layer that is cleaner or sharper than its surroundings stands out even when its colours are right.

## Copy the camera's blur & grain

Measures the background around the layer — its noise, softness and any motion blur — and copies them onto the layer (no AI). On a blurry, moving shot you get the same motion blur.

> **One-click fit** copies only the grain. Use this button when you also want the softness and motion blur.

## Settings

| Setting | What it does |
|---|---|
| Grain | film / sensor noise |
| Grain size | size of the grain specks (bigger for upscaled or old footage) |
| Black-and-white grain | grain without colour speckles |
| Blur | soften the layer (pixels) |
| Sharpen | crisper edges |
| Motion blur | streak length in pixels, like a moving camera |
| Motion direction | angle of the streak |
| Haze | distance fog: far-away things look paler and lower in contrast |
| Haze colour | colour of the fog (usually the sky or horizon colour) |
| Edge light | light from the background spilling over the layer's edges (*light wrap*); sells bright backgrounds |
| Edge light width | how far the edge light reaches in |
| Blur me like the background's depth of field | if the background has depth of field, the layer gets the same blur as the ground where it stands |

## Lens: depth of field, bloom, colour fringing

Mostly for backgrounds that look too clean, like game screenshots and 3D renders. These live on the background layer's inspector too.

- **Depth of field**: blurs what's far from the focus, like a real lens. It needs a depth map — click **Measure depth (AI)**. Then **Pick focus** and click what should be sharp. **Blur amount**, **Focus distance** and **Sharp range** fine-tune it.
- **Bloom**: glow around bright lights. **Bloom size** is how far it spreads, **Bloom from** how bright something must be to glow.
- **Colour fringing**: red/blue edges like a cheap lens (chromatic aberration).

> Less is more. Depth of field and bloom are easy to overdo; leave them off unless the background really needs them.
