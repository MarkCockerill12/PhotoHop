# Blender round trip

For things PhotoHop can't fake in 2D — a 3D prop in your hand, shadows on uneven ground, reflections — send your shot to Blender, add 3D objects there, render, and bring the render back.

## What you need

- Blender 5.1 or newer.
- The **PhotoHop Bridge** add-on: `photohop_bridge.zip` from the PhotoHop releases page. In Blender: *Edit > Preferences > Add-ons > Install from disk*.

## 1. Export to Blender

Select your layer (you) and choose **File > Export to Blender…**. Pick an empty folder. PhotoHop writes a *bundle*: your cut-out plate, the camera, the light from the Relight pad and, if you made them, the camera solve and body motion.

For moving shots, first run **AI > Work out the camera movement** so the Blender camera follows the phone's real movement. For props in your hands, run **AI > Capture body motion** (after removing the background of the clip).

## 2. In Blender

Load the bundle with the PhotoHop Bridge add-on. It builds the scene:

- a camera matching your shot,
- you as a card that casts shadows and hides props behind you,
- a shadow-catcher floor at your feet,
- a sun matching the Relight pad,
- with body motion: your animated 3D body, plus **PhotoHop right hand** / **PhotoHop left hand** empties — parent a prop to one and it follows your hand.

Add your props, then use the add-on's **Render for PhotoHop**.

## 3. Import the render

**File > Import Blender render…** and pick the bundle folder. The renders come back as layers (props, shadows) that you can colour-match and adjust like anything else.
