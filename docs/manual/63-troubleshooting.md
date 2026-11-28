# Troubleshooting

## The colour match makes my layer look strange

- Is the layer **above** the background in the Layers list? The match looks at what's below.
- Leftover background in the cut-out (studio grey, green spill) is measured as part of you. Clean the cut-out first.
- Several objects in one layer standing in different light? [Split them](several-objects) first.
- Lower **Match strength**, or try **Match with AI** instead (or the other way round).
- Strongly coloured objects keep their colour on purpose; only the light, levels and saturation change. See [Fit into the scene](fit#why-your-colours-stay-your-colours).

## The shadow is in the wrong place

- Click **Find feet**. If the cut-out includes floor or a reflection under the feet, trim it first.
- Drag the orange dot with the Move tool to aim it.
- On a slope, shorten the **Length**.

## A layer's name is red

Its file could not be loaded — usually it was moved or renamed. Hover the name to see the path. Put the file back, or re-import it.

## AI says it can't start

Open **AI > AI settings…** and check the Python path points at the environment where you installed the AI tools. See [AI tools](ai#setting-up-the-ai-tools). Without an NVIDIA card everything still works, just slower.

## Video playback is slow

**View > Video preview quality > Quarter**. Exports always use full quality.

## Everything looks too sharp / too clean

Use **Copy the camera's blur & grain** in [Camera & lens look](camera). Game screenshots may also need a little **Haze** on distant layers.

## I lost my work

PhotoHop doesn't auto-save. Save often with **Ctrl+S**. Undo (Ctrl+Z) goes back through every change in the session.
