# SPDX-License-Identifier: GPL-3.0-or-later
"""PhotoHop Bridge: build a Blender scene from a PhotoHop bundle and render props + shadows back.

The scene: a camera matching the PhotoHop canvas, the subject as a camera-facing card that casts
shadows and hides (holds out) anything behind it but is itself invisible, a shadow-catcher floor at
the subject's feet, and a sun matching PhotoHop's Relight pad. Add props, then Render for PhotoHop.

Headless use (testing / batch):
    blender -b --factory-startup --python __init__.py -- BUNDLE_DIR [--render] [--test-prop]
"""

import json
import math
import os
import sys

import bpy
from bpy.props import StringProperty
from bpy_extras.io_utils import ImportHelper
from mathutils import Vector

bl_info = {
    "name": "PhotoHop Bridge",
    "author": "PhotoHop",
    "version": (0, 2, 0),
    "blender": (4, 2, 0),
    "location": "File > Import > PhotoHop bundle; Render > Render for PhotoHop",
    "description": "Round-trip PhotoHop composites through Blender (props, shadows, reflections)",
    "category": "Import-Export",
}

DISTANCE = 5.0  # metres from camera to the subject card


def _load_manifest(path):
    folder = path if os.path.isdir(path) else os.path.dirname(path)
    with open(os.path.join(folder, "manifest.json"), encoding="utf8") as f:
        return folder, json.load(f)


def _sequence_image(folder, pattern, frames):
    """Load a printf-pattern image sequence as a Blender image (SEQUENCE source)."""
    first = os.path.join(folder, pattern.replace("%04d", "0001"))
    img = bpy.data.images.load(first, check_existing=True)
    if frames > 1:
        img.source = "SEQUENCE"
    return img


def _card_material(img, frames):
    mat = bpy.data.materials.new("PhotoHop subject card")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Linear"
    if frames > 1:
        tex.image_user.frame_duration = frames
        tex.image_user.frame_start = 1
        tex.image_user.use_auto_refresh = True
    transparent = nt.nodes.new("ShaderNodeBsdfTransparent")
    holdout = nt.nodes.new("ShaderNodeHoldout")
    diffuse = nt.nodes.new("ShaderNodeBsdfDiffuse")
    diffuse.inputs["Color"].default_value = (0.0, 0.0, 0.0, 1.0)
    camera_mix = nt.nodes.new("ShaderNodeMixShader")  # camera rays: hole where the subject is
    shadow_mix = nt.nodes.new("ShaderNodeMixShader")  # shadow rays: opaque where the subject is
    path = nt.nodes.new("ShaderNodeLightPath")
    final = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(tex.outputs["Alpha"], camera_mix.inputs["Fac"])
    nt.links.new(transparent.outputs[0], camera_mix.inputs[1])
    nt.links.new(holdout.outputs[0], camera_mix.inputs[2])
    nt.links.new(tex.outputs["Alpha"], shadow_mix.inputs["Fac"])
    nt.links.new(transparent.outputs[0], shadow_mix.inputs[1])
    nt.links.new(diffuse.outputs[0], shadow_mix.inputs[2])
    nt.links.new(path.outputs["Is Shadow Ray"], final.inputs["Fac"])
    nt.links.new(camera_mix.outputs[0], final.inputs[1])
    nt.links.new(shadow_mix.outputs[0], final.inputs[2])
    nt.links.new(final.outputs[0], out.inputs["Surface"])
    if hasattr(mat, "surface_render_method"):
        mat.surface_render_method = "BLENDED"
    return mat


def _collection(name):
    col = bpy.data.collections.get(name) or bpy.data.collections.new(name)
    if col.name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(col)
    return col


def build_scene(bundle):
    folder, m = _load_manifest(bundle)
    scene = bpy.context.scene
    w, h, fps, frames = int(m["width"]), int(m["height"]), float(m["fps"]), int(m["frames"])

    try:
        bpy.ops.preferences.addon_enable(module="cycles")
    except Exception as e:  # noqa: BLE001 - already enabled / built in
        print("PhotoHop Bridge: could not enable Cycles add-on:", e)
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 64
    scene.render.film_transparent = True
    scene.render.resolution_x, scene.render.resolution_y = w, h
    scene.render.resolution_percentage = 100
    scene.render.fps = max(1, round(fps))
    scene.render.fps_base = scene.render.fps / fps
    scene.frame_start, scene.frame_end = 1, frames
    ims = scene.render.image_settings
    if hasattr(ims, "media_type"):
        ims.media_type = "IMAGE"
    ims.file_format = "PNG"
    ims.color_mode = "RGBA"
    scene.render.filepath = os.path.join(folder, m.get("render", "render/render_"))
    scene.render.use_file_extension = True

    col = _collection("PhotoHop")
    _collection("Props")

    # Camera looking along +Y, horizon at the vertical centre of the frame.
    fov = math.radians(float(m.get("fov_deg", 50.0)))
    cam_data = bpy.data.cameras.new("PhotoHop camera")
    cam_data.sensor_fit = "AUTO"
    cam_data.angle = fov
    cam = bpy.data.objects.new("PhotoHop camera", cam_data)
    cam.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    col.objects.link(cam)
    scene.camera = cam

    # Size of the frame at the card distance (sensor fit AUTO = the larger side gets the angle).
    big = 2.0 * DISTANCE * math.tan(fov / 2.0)
    card_w, card_h = (big, big * h / w) if w >= h else (big * w / h, big)

    # Subject card filling the frame.
    bpy.ops.mesh.primitive_plane_add(size=1.0)
    card = bpy.context.active_object
    card.name = "PhotoHop subject"
    card.scale = (card_w, card_h, 1.0)
    card.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    card.location = (0.0, DISTANCE, 0.0)
    for c in list(card.users_collection):
        c.objects.unlink(card)
    col.objects.link(card)
    subject = _sequence_image(folder, m["subject"], frames)
    card.data.materials.append(_card_material(subject, frames))
    if hasattr(card, "visible_shadow"):
        card.visible_shadow = True

    # Shadow-catcher floor at the feet line.
    _fx, fy = m.get("feet_px", [w / 2, h])
    ground_z = (0.5 - float(fy) / h) * card_h
    bpy.ops.mesh.primitive_plane_add(size=200.0, location=(0.0, DISTANCE + 90.0, ground_z))
    floor = bpy.context.active_object
    floor.name = "PhotoHop shadow catcher"
    floor.is_shadow_catcher = True
    for c in list(floor.users_collection):
        c.objects.unlink(floor)
    col.objects.link(floor)

    # Camera background (viewport only) for lining props up with the scene.
    bg = _sequence_image(folder, m["background"], frames)
    cam_data.show_background_images = True
    bgi = cam_data.background_images.new()
    bgi.image = bg
    bgi.alpha = 1.0
    if frames > 1:
        bgi.image_user.frame_duration = frames
        bgi.image_user.use_auto_refresh = True

    # Sun from PhotoHop's light pad: camera space (x right, y up, z towards viewer) → world.
    light = m.get("light", {})
    az, el = math.radians(float(light.get("azimuth", 45.0))), math.radians(float(light.get("elevation", 35.0)))
    d_cam = Vector((math.cos(el) * math.cos(az), math.cos(el) * math.sin(az), math.sin(el)))
    to_light = Vector((d_cam.x, -d_cam.z, d_cam.y)).normalized()
    sun_data = bpy.data.lights.new("PhotoHop sun", type="SUN")
    sun_data.energy = 3.0 * float(light.get("intensity", 1.0))
    sun_data.color = tuple(light.get("color", [1.0, 1.0, 1.0]))[:3]
    sun_data.angle = math.radians(2.0)
    sun = bpy.data.objects.new("PhotoHop sun", sun_data)
    sun.rotation_euler = to_light.to_track_quat("Z", "Y").to_euler()
    col.objects.link(sun)

    world = scene.world or bpy.data.worlds.new("World")
    scene.world = world
    world.use_nodes = True
    bgnode = world.node_tree.nodes.get("Background")
    if bgnode:
        bgnode.inputs["Color"].default_value = (0.35, 0.35, 0.38, 1.0)
        bgnode.inputs["Strength"].default_value = 0.6
    scene["photohop_bundle"] = folder
    return folder


class PHOTOHOP_OT_import_bundle(bpy.types.Operator, ImportHelper):
    """Build a scene from a PhotoHop bundle (pick its manifest.json)"""

    bl_idname = "photohop.import_bundle"
    bl_label = "Import PhotoHop bundle"
    filename_ext = ".json"
    filter_glob: StringProperty(default="manifest.json", options={"HIDDEN"})

    def execute(self, context):
        folder = build_scene(self.filepath)
        self.report({"INFO"}, f"PhotoHop scene ready ({folder}). Add props to the 'Props' collection.")
        return {"FINISHED"}


class PHOTOHOP_OT_render(bpy.types.Operator):
    """Render props and shadows into the bundle's render folder"""

    bl_idname = "photohop.render"
    bl_label = "Render for PhotoHop"

    def execute(self, context):
        if "photohop_bundle" not in context.scene:
            self.report({"ERROR"}, "Import a PhotoHop bundle first")
            return {"CANCELLED"}
        bpy.ops.render.render(animation=True, write_still=False)
        self.report({"INFO"}, "Done. In PhotoHop: File > Import Blender render…")
        return {"FINISHED"}


def _menu_import(self, _context):
    self.layout.operator(PHOTOHOP_OT_import_bundle.bl_idname, text="PhotoHop bundle (manifest.json)")


def _menu_render(self, _context):
    self.layout.operator(PHOTOHOP_OT_render.bl_idname)


CLASSES = (PHOTOHOP_OT_import_bundle, PHOTOHOP_OT_render)


def register():
    for c in CLASSES:
        bpy.utils.register_class(c)
    bpy.types.TOPBAR_MT_file_import.append(_menu_import)
    bpy.types.TOPBAR_MT_render.append(_menu_render)


def unregister():
    bpy.types.TOPBAR_MT_render.remove(_menu_render)
    bpy.types.TOPBAR_MT_file_import.remove(_menu_import)
    for c in reversed(CLASSES):
        bpy.utils.unregister_class(c)


def _cli():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if not argv:
        return
    bpy.ops.wm.read_factory_settings(use_empty=True)
    folder = build_scene(argv[0])
    if "--test-prop" in argv:
        # A bright cube standing on the floor to the subject's side, to check shadows + holdout.
        cam_h = bpy.context.scene.objects["PhotoHop shadow catcher"].location.z
        bpy.ops.mesh.primitive_cube_add(size=0.5, location=(-0.6, 5.6, cam_h + 0.25))
        cube = bpy.context.active_object
        mat = bpy.data.materials.new("test")
        mat.use_nodes = True
        mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.9, 0.2, 0.1, 1.0)
        cube.data.materials.append(mat)
    if "--render" in argv:
        sc = bpy.context.scene
        if "--first-frame" in argv:
            sc.frame_end = sc.frame_start
        sc.cycles.samples = 32
        bpy.ops.render.render(animation=True)
    print("PHOTOHOP_BRIDGE_OK", folder)


if __name__ == "__main__":
    _cli()
